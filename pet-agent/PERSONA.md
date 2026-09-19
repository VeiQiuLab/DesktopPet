# Persona

桌宠「是谁」——身份设定、说话风格、基础行为原则。属于 pet-agent，与 Live2D Character（长什么样）解耦。

## 目录

```
pet-agent/personas/<id>/persona.json
```

## Schema

```json
{
  "id": "default",
  "name": "Default",
  "system_prompt": "你是桌面角色的对话层……1～3 句……不要 Markdown……",
  "style": { "max_sentences": 3, "prefer_concise": true }
}
```

## 校验

扫描 personas 时验证：persona.json 存在、id/name/system_prompt 非空、style.max_sentences ∈ [1,20]。
坏 Persona 跳过并记 warning，不导致 Agent 启动失败。

## 切换

- CLI：`pet-agent personas`（列出）/ `pet-agent persona <id>`（切换）
- Tray：`Persona` 子菜单（当前带勾选）
- 配置：`pet-agent.config.json` 的 `active_persona`（缺失 fallback default；不存在 fallback default + warning）

**切换行为**：更新 active persona + **清空短期 Conversation**；
**不**删除 Memory、**不**修改 DesktopPet Character、**不**影响 TTS Provider。

## 边界

| | 含义 | 归属 |
|---|---|---|
| Persona | 桌宠是谁 | pet-agent |
| Memory | 桌宠知道什么 | pet-agent |
| Conversation | 刚才聊了什么 | pet-agent（临时） |
| Character | 长什么样 | DesktopPet |

Memory 默认**跨 Persona 共用**（本阶段不做每 Persona 独立 Memory）。

## 示例

`default`（3 句、简洁）与 `quiet`（1 句、话少）用于验证切换。
