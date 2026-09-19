# Memory V1

桌面宠物的长期记忆：**可控、可查看、可修改、可删除**。

## 架构

```
User → ConversationCoordinator
        ├─ Persona（桌宠是谁）
        ├─ Short-term Conversation（刚才聊了什么，重启丢失）
        └─ Long-term Memory（桌宠知道什么，持久化）
              ↓
        PromptBuilder → AI Provider
```

仅存在于 pet-agent。DesktopPet 不知道 Persona / Memory / Prompt / DB。

## 存储

- SQLite：`pet-agent/data/memory.db`（exe 同目录 `data/`）
- WAL 模式；`meta.schema_version = 1`
- **不提交 Git**（.gitignore 已排除）
- 表：`memories` / `pending_changes` / `audit_log` / `meta`

## Memory schema

```
memories: id, kind, content, created_at, updated_at, source, status(active|deleted), pinned
pending_changes: id, action(create|update|delete), kind, content, target_id, created_at, source
audit_log: id, ts, action, memory_id, before, after, source
```

kind：`fact` / `preference` / `project` / `relationship` / `custom`

## 原则

- **显式记忆**：「记住：X」→ 直接写 active（有审计日志）
- **隐式建议**：普通聊天推测 → `pending_changes`（不直接 active）
- **冲突**：新信息与旧记忆冲突 → `pending update`（不静默覆盖）
- **删除**：仅用户明确删除 / 确认 pending；soft delete（status=deleted），保留审计
- **不自动删除**：不按时间/置信度/频率/AI 判断后台永久删除
- **AI 无 DB 权限**：LLM 只能产生 proposal，DB 操作由本地可信代码执行

## PromptBuilder

顺序：`System Persona` → `[Relevant user memory]`（明确标注为背景数据，非指令）→ 短期对话 → 当前 user。
Persona System Prompt 永远高于 memory（防 prompt injection）。

## Retrieval V1

无 Embedding。规则：pinned 优先 + keyword 匹配 + recency；`max=12 条 / max=1200 字符`（context budget）。

## CLI

```
pet-agent memory list
pet-agent memory pending
pet-agent memory accept <id>
pet-agent memory reject <id>
pet-agent memory delete <id>
pet-agent memory export
pet-agent memory import <file.json>
pet-agent memory backup
pet-agent memory audit
```

## Privacy

Memory **只存本地**；不上传、不云同步、不遥测。
⚠️ 注意：调用**云端** LLM 时，为生成回复而注入的 relevant memories 会随 prompt 发给 Provider。
Provider 为本地时不离开本机。

## Failure degradation

DB 打不开 / migration 失败 → `memory unavailable`，AI 退化为**无记忆模式**，仍可聊天；不影响 DesktopPet。

## 未来 Memory V2 扩展点

- Embedding / 向量检索（替换 retrieval V1）
- pending 确认 UI（当前以 CLI 为主）
- import（当前 export 已就绪）
- 多 Persona 切换
