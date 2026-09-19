//! Memory V1：可控、可查看、可修改、可删除的长期记忆。
//!
//! 原则：
//! - 用户可查看 / 修改 / 删除
//! - 不自动删除、不静默覆盖（冲突进入 pending）
//! - 所有变化写 audit log
//! - LLM 不能直接操作数据库（只能产生 proposal，由本地可信代码执行）
//! - DB 失败时降级为「无记忆模式」，不影响聊天

use std::path::PathBuf;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub mod intent;

pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
    pub source: String,
    pub status: String, // active | deleted
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingChange {
    pub id: i64,
    pub action: String, // create | update | delete
    pub kind: String,
    pub content: String,
    pub target_id: Option<i64>,
    pub created_at: String,
    pub source: String,
}

pub struct MemoryManager {
    conn: Connection,
}

impl MemoryManager {
    /// 打开（或创建）数据库并执行 migration。
    pub fn open(path: &PathBuf) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path).map_err(|e| format!("open db failed: {e}"))?;
        let mgr = MemoryManager { conn };
        mgr.migrate()?;
        Ok(mgr)
    }

    fn migrate(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
                 CREATE TABLE IF NOT EXISTS memories (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     kind TEXT NOT NULL,
                     content TEXT NOT NULL,
                     created_at TEXT NOT NULL,
                     updated_at TEXT NOT NULL,
                     source TEXT NOT NULL,
                     status TEXT NOT NULL DEFAULT 'active',
                     pinned INTEGER NOT NULL DEFAULT 0
                 );
                 CREATE TABLE IF NOT EXISTS pending_changes (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     action TEXT NOT NULL,
                     kind TEXT NOT NULL,
                     content TEXT NOT NULL,
                     target_id INTEGER,
                     created_at TEXT NOT NULL,
                     source TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS audit_log (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     ts TEXT NOT NULL,
                     action TEXT NOT NULL,
                     memory_id INTEGER,
                     before TEXT,
                     after TEXT,
                     source TEXT NOT NULL
                 );",
            )
            .map_err(|e| format!("migrate failed: {e}"))?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
                [SCHEMA_VERSION.to_string()],
            )
            .map_err(|e| format!("set version failed: {e}"))?;
        Ok(())
    }

    fn now() -> String {
        chrono::Local::now().to_rfc3339()
    }

    fn audit(
        &self,
        action: &str,
        memory_id: Option<i64>,
        before: Option<&str>,
        after: Option<&str>,
        source: &str,
    ) {
        let _ = self.conn.execute(
            "INSERT INTO audit_log (ts, action, memory_id, before, after, source) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![Self::now(), action, memory_id, before, after, source],
        );
    }

    /// 直接创建 active memory（用于显式「记住……」）。
    pub fn create(&self, kind: &str, content: &str, source: &str) -> Result<i64, String> {
        let now = Self::now();
        self.conn
            .execute(
                "INSERT INTO memories (kind, content, created_at, updated_at, source, status, pinned) VALUES (?1,?2,?3,?4,?5,'active',0)",
                rusqlite::params![kind, content, now, now, source],
            )
            .map_err(|e| format!("insert failed: {e}"))?;
        let id = self.conn.last_insert_rowid();
        self.audit("create", Some(id), None, Some(content), source);
        Ok(id)
    }

    /// 创建 pending（用于 implicit suggestion / 冲突 update）。
    pub fn propose(
        &self,
        action: &str,
        kind: &str,
        content: &str,
        target_id: Option<i64>,
        source: &str,
    ) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO pending_changes (action, kind, content, target_id, created_at, source) VALUES (?1,?2,?3,?4,?5,?6)",
                rusqlite::params![action, kind, content, target_id, Self::now(), source],
            )
            .map_err(|e| format!("propose failed: {e}"))?;
        let id = self.conn.last_insert_rowid();
        self.audit("propose", target_id, None, Some(content), source);
        Ok(id)
    }

    /// 列出 active memories。
    pub fn list_active(&self) -> Vec<Memory> {
        self.query(
            "SELECT id,kind,content,created_at,updated_at,source,status,pinned FROM memories WHERE status='active' ORDER BY pinned DESC, updated_at DESC",
        )
    }

    /// 列出 pending。
    pub fn list_pending(&self) -> Vec<PendingChange> {
        let mut out = Vec::new();
        let mut stmt = match self.conn.prepare(
            "SELECT id,action,kind,content,target_id,created_at,source FROM pending_changes ORDER BY created_at DESC",
        ) {
            Ok(s) => s,
            Err(_) => return out,
        };
        let rows = stmt.query_map([], |r| {
            Ok(PendingChange {
                id: r.get(0)?,
                action: r.get(1)?,
                kind: r.get(2)?,
                content: r.get(3)?,
                target_id: r.get(4)?,
                created_at: r.get(5)?,
                source: r.get(6)?,
            })
        });
        if let Ok(rows) = rows {
            for r in rows.flatten() {
                out.push(r);
            }
        }
        out
    }

    fn query(&self, sql: &str) -> Vec<Memory> {
        let mut out = Vec::new();
        let mut stmt = match self.conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return out,
        };
        let rows = stmt.query_map([], |r| {
            Ok(Memory {
                id: r.get(0)?,
                kind: r.get(1)?,
                content: r.get(2)?,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
                source: r.get(5)?,
                status: r.get(6)?,
                pinned: r.get::<_, i64>(7)? != 0,
            })
        });
        if let Ok(rows) = rows {
            for r in rows.flatten() {
                out.push(r);
            }
        }
        out
    }

    /// 接受 pending。
    pub fn accept(&self, pending_id: i64) -> Result<(), String> {
        let p = self
            .list_pending()
            .into_iter()
            .find(|p| p.id == pending_id)
            .ok_or_else(|| "pending not found".to_string())?;
        match p.action.as_str() {
            "create" => {
                self.create(&p.kind, &p.content, "accepted")?;
            }
            "update" => {
                if let Some(tid) = p.target_id {
                    self.update_content(tid, &p.content)?;
                }
            }
            "delete" => {
                if let Some(tid) = p.target_id {
                    self.soft_delete(tid)?;
                }
            }
            _ => {}
        }
        let _ = self
            .conn
            .execute("DELETE FROM pending_changes WHERE id=?1", [pending_id]);
        Ok(())
    }

    /// 拒绝 pending。
    pub fn reject(&self, pending_id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM pending_changes WHERE id=?1", [pending_id])
            .map_err(|e| format!("reject failed: {e}"))?;
        self.audit("reject", None, None, None, "user");
        Ok(())
    }

    /// 更新内容（显式修改 / 接受 update）。
    pub fn update_content(&self, id: i64, new_content: &str) -> Result<(), String> {
        let before: Option<String> = self
            .conn
            .query_row("SELECT content FROM memories WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .ok();
        self.conn
            .execute(
                "UPDATE memories SET content=?1, updated_at=?2 WHERE id=?3",
                rusqlite::params![new_content, Self::now(), id],
            )
            .map_err(|e| format!("update failed: {e}"))?;
        self.audit(
            "update",
            Some(id),
            before.as_deref(),
            Some(new_content),
            "user",
        );
        Ok(())
    }

    /// 软删除（保留审计信息）。
    pub fn soft_delete(&self, id: i64) -> Result<(), String> {
        let before: Option<String> = self
            .conn
            .query_row("SELECT content FROM memories WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .ok();
        self.conn
            .execute(
                "UPDATE memories SET status='deleted', updated_at=?1 WHERE id=?2",
                rusqlite::params![Self::now(), id],
            )
            .map_err(|e| format!("delete failed: {e}"))?;
        self.audit("delete", Some(id), before.as_deref(), None, "user");
        Ok(())
    }

    /// 设置 pinned。
    #[allow(dead_code)]
    pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE memories SET pinned=?1, updated_at=?2 WHERE id=?3",
                rusqlite::params![pinned as i64, Self::now(), id],
            )
            .map_err(|e| format!("pin failed: {e}"))?;
        self.audit(
            "pin",
            Some(id),
            None,
            Some(if pinned { "pinned" } else { "unpinned" }),
            "user",
        );
        Ok(())
    }

    /// 检索：pinned 优先 + keyword 匹配 + recency；受 max 限制。
    pub fn retrieve(&self, query: &str, max: usize, max_chars: usize) -> Vec<Memory> {
        let all = self.list_active();
        let q = query.to_lowercase();
        let mut scored: Vec<(i32, Memory)> = all
            .into_iter()
            .map(|m| {
                let mut score = 0;
                if m.pinned {
                    score += 100;
                }
                let c = m.content.to_lowercase();
                for kw in q.split_whitespace() {
                    if !kw.is_empty() && c.contains(kw) {
                        score += 10;
                    }
                }
                (score, m)
            })
            .collect();
        // pinned / 关键词优先；否则按时间（list_active 已按 updated_at DESC）
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        let mut out = Vec::new();
        let mut chars = 0usize;
        for (_, m) in scored {
            if out.len() >= max {
                break;
            }
            chars += m.content.chars().count();
            if chars > max_chars {
                break;
            }
            out.push(m);
        }
        out
    }

    /// 查找可能冲突的 active memory（共享 ≥2 个「双字/词」即视为冲突）。
    pub fn find_conflict(&self, content: &str) -> Option<Memory> {
        let target = bigrams(content);
        for m in self.list_active() {
            let src = bigrams(&m.content);
            let overlap = target.iter().filter(|g| src.contains(g)).count();
            if overlap >= 2 {
                return Some(m);
            }
        }
        None
    }

    /// 最近更新的 active memory（change 意图的回退目标）。
    pub fn most_recent_active(&self) -> Option<Memory> {
        self.list_active().into_iter().next()
    }

    /// 导出全部（active + deleted）为 JSON。
    pub fn export_json(&self) -> String {
        let all = self.query(
            "SELECT id,kind,content,created_at,updated_at,source,status,pinned FROM memories ORDER BY id",
        );
        serde_json::to_string_pretty(&all).unwrap_or_else(|_| "[]".into())
    }

    /// 备份数据库文件。
    pub fn backup(&self, src: &PathBuf) -> Result<PathBuf, String> {
        let mut backups = src
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        backups.push("backups");
        let _ = std::fs::create_dir_all(&backups);
        let name = format!("memory_{}.db", chrono::Local::now().format("%Y%m%d_%H%M%S"));
        let dst = backups.join(name);
        std::fs::copy(src, &dst).map_err(|e| format!("backup failed: {e}"))?;
        Ok(dst)
    }

    // (helper appended below)

    /// 审计日志（供查看）。
    pub fn audit_log(
        &self,
        limit: usize,
    ) -> Vec<(String, String, Option<i64>, Option<String>, Option<String>)> {
        let mut out = Vec::new();
        let mut stmt = match self.conn.prepare(
            "SELECT ts,action,memory_id,before,after FROM audit_log ORDER BY id DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return out,
        };
        let rows = stmt.query_map([limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        });
        if let Ok(rows) = rows {
            for r in rows.flatten() {
                out.push(r);
            }
        }
        out
    }
}

/// 提取双字（CJK 相邻两字）/ ASCII 词作为重叠单位。
fn bigrams(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    // CJK bigrams
    for w in chars.windows(2) {
        let a = w[0];
        let b = w[1];
        if ('一'..='鿿').contains(&a) && ('一'..='鿿').contains(&b) {
            out.push(format!("{a}{b}"));
        }
    }
    // ASCII words
    for tok in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if tok.chars().count() >= 3 {
            out.push(tok.to_lowercase());
        }
    }
    out
}
