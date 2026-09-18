//! CharacterManager：扫描 / 验证 / 提供角色包。
//!
//! 职责：
//! - 扫描 characters/ 目录，发现合法角色包
//! - 解析 character.json、验证模型入口
//! - 加载指定角色并暴露给应用层
//!
//! 扫描只在启动（或显式刷新）时执行；不每帧扫描、不重复加载。
//!
//! 容错：单个坏角色包不会中断整个扫描。

use std::path::{Path, PathBuf};

use crate::config::log_line;

use super::package::CharacterPackage;

pub struct CharacterManager {
    /// characters/ 根目录。
    #[allow(dead_code)]
    root: PathBuf,
    /// 已发现并验证通过的角色包（按目录名排序）。
    packages: Vec<CharacterPackage>,
}

impl CharacterManager {
    /// 扫描 root 下的所有角色包。坏角色会被记录日志并跳过。
    pub fn scan(root: &Path) -> Self {
        let mut packages = Vec::new();

        if !root.is_dir() {
            log_line(&format!(
                "characters root not found: {} (no characters loaded)",
                root.display()
            ));
            return CharacterManager {
                root: root.to_path_buf(),
                packages,
            };
        }

        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(e) => {
                log_line(&format!("read_dir {} failed: {e}", root.display()));
                return CharacterManager {
                    root: root.to_path_buf(),
                    packages,
                };
            }
        };

        let mut dirs: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                dirs.push(p);
            }
        }
        dirs.sort();

        for dir in dirs {
            match CharacterPackage::load(&dir) {
                Ok(pkg) => {
                    log_line(&format!(
                        "character discovered: id={} name={} scale={} model3={}",
                        pkg.id(),
                        pkg.display_name(),
                        pkg.scale,
                        pkg.model3.display()
                    ));
                    packages.push(pkg);
                }
                Err(e) => {
                    log_line(&format!("character load failed ({}): {e}", dir.display()));
                }
            }
        }

        log_line(&format!(
            "character scan done: {} valid package(s)",
            packages.len()
        ));
        CharacterManager {
            root: root.to_path_buf(),
            packages,
        }
    }

    /// 所有已发现角色。
    #[allow(dead_code)]
    pub fn list(&self) -> &[CharacterPackage] {
        &self.packages
    }

    /// 按 id 查找。
    pub fn get(&self, id: &str) -> Option<&CharacterPackage> {
        self.packages.iter().find(|p| p.id() == id)
    }

    /// 选择激活角色：优先 id，找不到则 fallback 到第一个；无角色则 None。
    pub fn resolve_active(&self, preferred_id: &str) -> Option<&CharacterPackage> {
        if let Some(p) = self.get(preferred_id) {
            return Some(p);
        }
        if !preferred_id.is_empty() {
            log_line(&format!(
                "active character '{}' not found, falling back to first",
                preferred_id
            ));
        }
        self.packages.first()
    }

    #[allow(dead_code)]
    pub fn root(&self) -> &Path {
        &self.root
    }
}
