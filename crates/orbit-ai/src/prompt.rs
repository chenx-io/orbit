//! System prompts and context injection.
//!
//! Design: **prompts hold only the rules and the "current selection", never the full data**.
//! Large objects (collections/requests/scenarios) are fetched on demand by the model via read tools, so every turn doesn't burn a lot of tokens.

use serde::{Deserialize, Serialize};

use crate::mode::AiMode;

/// Prompt language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Simplified Chinese.
    Zh,
    /// English.
    En,
}

impl Language {
    /// Parse from a BCP-47 style string (`zh-CN` / `en-US`); defaults to Chinese.
    pub fn from_tag(tag: &str) -> Self {
        if tag.to_ascii_lowercase().starts_with("en") {
            Language::En
        } else {
            Language::Zh
        }
    }
}

/// Currently selected entity (context carried while the frontend drawer is open).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    /// Kind: `request` / `scenario` / `loadTest` / `collection`.
    pub kind: String,
    /// Entity id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Extra description (e.g. `POST https://…`).
    #[serde(default)]
    pub detail: String,
}

/// Objects the user **explicitly referenced** (the reference chips above the input box).
///
/// Unlike [`Selection`] (the "current selection" the frontend adds automatically), these are **boundaries** the user pinned deliberately -
/// "only add requests to this collection", "generate from this definition file" - so the prompt must state them as constraints,
/// not merely as background information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    /// Kind: `workspace` / `collection` / `request` / `scenario` / `file`.
    pub kind: String,
    /// Entity id (for `file`, the absolute file path).
    pub id: String,
    /// Display name (entity name / file name).
    pub name: String,
    /// Extra description (e.g. `POST /api/login`, file size).
    #[serde(default)]
    pub detail: String,
    /// **`file` kind only**: the definition text already read (may be truncated).
    ///
    /// The file body is inlined into the prompt directly, instead of giving the model a "read any path" tool -
    /// the latter hands the model read access to the whole filesystem (a single prompt injection can read any file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Context summary injected into the prompt (**metadata only, no secret values**).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSummary {
    /// Workspace id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Workspace name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    /// Active environment id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Active environment name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_name: Option<String>,
    /// Environment variable names (**no values**).
    #[serde(default)]
    pub variable_names: Vec<String>,
    /// Environment secret names (**no values**).
    #[serde(default)]
    pub secret_names: Vec<String>,
    /// Collection summaries (`id | name`).
    #[serde(default)]
    pub collections: Vec<String>,
    /// Total request count.
    #[serde(default)]
    pub request_count: usize,
    /// Total scenario count.
    #[serde(default)]
    pub scenario_count: usize,
    /// Total suite count.
    #[serde(default)]
    pub suite_count: usize,
    /// Data model names.
    #[serde(default)]
    pub model_names: Vec<String>,
    /// Data source names (usable in DB/Redis assertions).
    #[serde(default)]
    pub data_source_names: Vec<String>,
    /// Currently selected entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    /// Objects explicitly referenced by the user (boundary conditions).
    #[serde(default)]
    pub references: Vec<Reference>,
}

/// Build the system prompt: general principles + **behavior constraints for the current mode** + context summary.
///
/// The mode is not optional: the same request ("paginate the login request") should be answered with "how to do it" in Ask mode,
/// should produce a plan in Plan mode, and only actually act in Agent mode. Tool visibility is enforced by
/// [`crate::agent::Agent`]; the prompt only makes the model **aware** of which tier it is limited to.
pub fn system_prompt(lang: Language, ctx: &ContextSummary, mode: AiMode) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(match lang {
        Language::Zh => ZH_RULES,
        Language::En => EN_RULES,
    });
    out.push_str("\n\n");
    out.push_str(&mode_block(lang, mode));
    out.push_str("\n\n");
    // Syntax reference: dynamic value catalog / official assertion comparator names / script API list.
    // Placed after the mode constraints and before the context - handy when generating request definitions, without crowding out attention on the behavior constraints.
    out.push_str(&crate::syntax::syntax_reference(lang));
    out.push_str("\n\n");
    out.push_str(&render_context(lang, ctx));
    out
}

/// Render the "current mode" constraint block.
pub fn mode_block(lang: Language, mode: AiMode) -> String {
    match (lang, mode) {
        (Language::Zh, AiMode::Ask) => ASK_BLOCK_ZH.to_string(),
        (Language::Zh, AiMode::Agent) => AGENT_BLOCK_ZH.to_string(),
        (Language::Zh, AiMode::Plan) => PLAN_BLOCK_ZH.to_string(),
        (Language::En, AiMode::Ask) => ASK_BLOCK_EN.to_string(),
        (Language::En, AiMode::Agent) => AGENT_BLOCK_EN.to_string(),
        (Language::En, AiMode::Plan) => PLAN_BLOCK_EN.to_string(),
    }
}

/// Render the context block.
///
/// Every label is selected by `lang`; the Chinese branch is byte-for-byte unchanged so existing
/// Chinese prompts keep working.
pub fn render_context(lang: Language, ctx: &ContextSummary) -> String {
    let title = match lang {
        Language::Zh => "## 当前上下文\n",
        Language::En => "## Current context\n",
    };
    let mut lines = vec![title.to_string()];
    let ws = match (&ctx.workspace_name, &ctx.workspace_id) {
        (Some(name), Some(id)) => format!("{name} (id={id})"),
        (Some(name), None) => name.clone(),
        (None, Some(id)) => format!("(id={id})"),
        (None, None) => "-".to_string(),
    };
    match lang {
        Language::Zh => {
            lines.push(format!("- 工作区 workspace: {ws}\n"));
            match (&ctx.environment_name, &ctx.environment_id) {
                (Some(name), Some(id)) => {
                    lines.push(format!("- 激活环境 environment: {name} (id={id})\n"))
                }
                _ => lines.push("- 激活环境 environment: 未选择\n".to_string()),
            }
            if !ctx.variable_names.is_empty() {
                lines.push(format!(
                    "- 环境变量名 variables(仅名称): {}\n",
                    ctx.variable_names.join(", ")
                ));
            }
            if !ctx.secret_names.is_empty() {
                lines.push(format!(
                    "- 环境密钥名 secrets(仅名称，值不可见): {}\n",
                    ctx.secret_names.join(", ")
                ));
            }
            lines.push(format!(
                "- 集合数量 {}，接口 {} 个，用例 {} 个，套件 {} 个\n",
                ctx.collections.len(),
                ctx.request_count,
                ctx.scenario_count,
                ctx.suite_count
            ));
            if !ctx.collections.is_empty() {
                let shown = ctx.collections.iter().take(30).cloned().collect::<Vec<_>>();
                lines.push(format!("- 集合清单: {}\n", shown.join("; ")));
            }
            if !ctx.model_names.is_empty() {
                lines.push(format!("- 数据模型: {}\n", ctx.model_names.join(", ")));
            }
            if !ctx.data_source_names.is_empty() {
                lines.push(format!(
                    "- 数据源(可用于 db/redis 断言): {}\n",
                    ctx.data_source_names.join(", ")
                ));
            }
            match &ctx.selection {
                Some(s) => lines.push(format!(
                    "- 用户当前选中的{}: {} (id={}) {}\n",
                    entity_label(lang, &s.kind),
                    s.name,
                    s.id,
                    s.detail
                )),
                None => lines.push("- 用户当前未选中任何实体\n".to_string()),
            }
        }
        Language::En => {
            lines.push(format!("- Workspace: {ws}\n"));
            match (&ctx.environment_name, &ctx.environment_id) {
                (Some(name), Some(id)) => {
                    lines.push(format!("- Active environment: {name} (id={id})\n"))
                }
                _ => lines.push("- Active environment: none selected\n".to_string()),
            }
            if !ctx.variable_names.is_empty() {
                lines.push(format!(
                    "- Variable names (names only): {}\n",
                    ctx.variable_names.join(", ")
                ));
            }
            if !ctx.secret_names.is_empty() {
                lines.push(format!(
                    "- Secret names (names only, values hidden): {}\n",
                    ctx.secret_names.join(", ")
                ));
            }
            lines.push(format!(
                "- Collections {}, requests {}, scenarios {}, suites {}\n",
                ctx.collections.len(),
                ctx.request_count,
                ctx.scenario_count,
                ctx.suite_count
            ));
            if !ctx.collections.is_empty() {
                let shown = ctx.collections.iter().take(30).cloned().collect::<Vec<_>>();
                lines.push(format!("- Collections: {}\n", shown.join("; ")));
            }
            if !ctx.model_names.is_empty() {
                lines.push(format!("- Data models: {}\n", ctx.model_names.join(", ")));
            }
            if !ctx.data_source_names.is_empty() {
                lines.push(format!(
                    "- Data sources (usable in db/redis assertions): {}\n",
                    ctx.data_source_names.join(", ")
                ));
            }
            match &ctx.selection {
                Some(s) => lines.push(format!(
                    "- Current selection {}: {} (id={}) {}\n",
                    entity_label(lang, &s.kind),
                    s.name,
                    s.id,
                    s.detail
                )),
                None => lines.push("- No entity currently selected by the user\n".to_string()),
            }
        }
    }
    lines.push(render_references(lang, &ctx.references));
    lines.concat()
}

/// Render the "objects explicitly referenced by the user" block (including referenced file bodies).
///
/// This is a **constraint**, not background: if the user pinned "collection A", collection B must not be
/// touched. Returns an empty string when there are no references, costing no tokens.
pub fn render_references(lang: Language, refs: &[Reference]) -> String {
    if refs.is_empty() {
        return String::new();
    }
    let mut out = String::from(match lang {
        Language::Zh => "\n## 用户显式引用的对象（本次任务的边界）\n",
        Language::En => "\n## Objects explicitly referenced by the user (boundary of this task)\n",
    });
    out.push_str(match lang {
        Language::Zh => {
            "只操作下列对象；用户没有引用到的集合/接口/用例不要改动。被引用的定义文件是权威依据。\n"
        }
        Language::En => "Only operate on the objects listed below; do not modify collections/requests/scenarios \
the user did not reference. Referenced definition files are authoritative.\n",
    });
    for r in refs {
        let detail = if r.detail.trim().is_empty() {
            String::new()
        } else {
            format!(" {}", r.detail.trim())
        };
        match (lang, r.kind.as_str()) {
            (Language::Zh, "workspace") => out.push_str(&format!(
                "- 工作区「{}」(id={})：本次任务只在这个工作区内操作\n",
                r.name, r.id
            )),
            (Language::Zh, "collection") => out.push_str(&format!(
                "- 集合「{}」(id={})：只在这个集合内新建/修改接口\n",
                r.name, r.id
            )),
            (Language::Zh, "request") => out.push_str(&format!(
                "- 接口「{}」(id={}){}：以它为准，不要另建同名接口\n",
                r.name, r.id, detail
            )),
            (Language::Zh, "scenario") => out.push_str(&format!(
                "- 自动化用例「{}」(id={}){}：围绕它编排或修改\n",
                r.name, r.id, detail
            )),
            (Language::Zh, "file") => {
                let note = if r.content.is_some() {
                    "：正文见下方「引用文件」"
                } else {
                    "：文件内容未能读取"
                };
                out.push_str(&format!(
                    "- 接口定义文件「{}」{}{}\n",
                    r.name, detail, note
                ));
            }
            (Language::Zh, other) => {
                out.push_str(&format!("- {}「{}」(id={}){}\n", other, r.name, r.id, detail))
            }
            (Language::En, "workspace") => out.push_str(&format!(
                "- Workspace \"{}\" (id={}): operate only within this workspace for this task\n",
                r.name, r.id
            )),
            (Language::En, "collection") => out.push_str(&format!(
                "- Collection \"{}\" (id={}): create/modify requests only within this collection\n",
                r.name, r.id
            )),
            (Language::En, "request") => out.push_str(&format!(
                "- Request \"{}\" (id={}){}: treat it as authoritative, do not create another request with the same name\n",
                r.name, r.id, detail
            )),
            (Language::En, "scenario") => out.push_str(&format!(
                "- Scenario \"{}\" (id={}){}: orchestrate or modify around it\n",
                r.name, r.id, detail
            )),
            (Language::En, "file") => {
                let note = if r.content.is_some() {
                    ": body appears below under \"Referenced files\""
                } else {
                    ": file content could not be read"
                };
                out.push_str(&format!(
                    "- Request definition file \"{}\"{}{}\n",
                    r.name, detail, note
                ));
            }
            (Language::En, other) => {
                out.push_str(&format!("- {} \"{}\" (id={}){}\n", other, r.name, r.id, detail))
            }
        }
    }
    for r in refs.iter().filter(|r| r.kind == "file") {
        if let Some(content) = r.content.as_deref().filter(|c| !c.trim().is_empty()) {
            let head = match lang {
                Language::Zh => "### 引用文件：",
                Language::En => "### Referenced file: ",
            };
            out.push_str(&format!(
                "\n{head}{}\n```\n{}\n```\n",
                r.name,
                content.trim()
            ));
        }
    }
    out
}

/// Label used for a referenced/selected entity kind, selected by prompt language.
fn entity_label(lang: Language, kind: &str) -> &'static str {
    match (lang, kind) {
        (Language::Zh, "request") => "接口",
        (Language::Zh, "scenario") => "用例",
        (Language::Zh, "loadTest") => "压测目标",
        (Language::Zh, "collection") => "集合",
        (Language::Zh, _) => "实体",
        (Language::En, "request") => "request",
        (Language::En, "scenario") => "scenario",
        (Language::En, "loadTest") => "load test target",
        (Language::En, "collection") => "collection",
        (Language::En, _) => "entity",
    }
}

const ZH_RULES: &str = r#"你是 Orbit（API 测试工具）内置的 AI 助手，帮助用户把自然语言或接口描述变成可用的接口资产，并驱动工程内的执行引擎完成调试、自动化与压测。

## 工作原则
1. **先读后写**：任何要引用 id 的操作（改接口、建用例、跑接口）都必须先用读取类工具（list_collections / list_requests / get_request / list_scenarios）拿到真实 id，**绝不编造 id**。
2. **只做被允许的事**：你的可用工具由当前模式决定（见下文「当前模式」）。工具清单里没有的能力就是不允许的，不要假装自己做过，也不要用文字描述冒充执行结果。
3. **修改用补丁**：update_request / update_scenario 只传要改的字段（JSON Merge Patch；字段置 null 表示删除），不要重写整个对象。改**单个动作**（如某个数据库动作的 SQL、某段脚本的代码）必须用动作级工具 update_action / insert_action / delete_action / move_action（`list` + `index` 定位，先用 get_request 读当前列表）—— 用 update_request 重发 `preActions` 是**整表替换**，极易丢字段。
4. **严谨的脚本与断言**：脚本用 `pm.*`、断言用 `assertions` 数组；**具体写法一律照下文「语法参考」**（那是唯一权威，含比较器官方名与脚本 API 清单——不要凭其他产品的记忆写）。两条硬规则：**时长必须带单位**（`"2s"` / `"800ms"`，纯数字会被当成秒，差 1000 倍）；**带请求体时三件套缺一不可**：`bodyMode`（如 `json`）+ `body`（字符串）+ `contentType`（如 `application/json`）——只给 `body` 会被引擎忽略，请求会以空体发出。
5. **变量与动态值**：接口 URL、Header、Body、脚本中可直接写 `{{变量名}}`（环境变量/全局变量）或动态值 `{{$category.method}}`；执行时由引擎统一插值，**不要自己先替换成具体值**；**动态值只能用「语法参考」里列出的真实 token**，写错会被原样发送（`{{$uuid.v4}}`、`{{$timestamp.ms}}`、`{{$randomInt}}` 这类 Postman 写法在本产品里不存在）。
6. **安全**：环境密钥的值对你不可见（只给名称）。不要要求用户把 Token/密码贴进对话；需要鉴权时用 `{{变量名}}` 或 auth 配置引用。
7. **失败要解释**：拿到工具结果后，用简洁中文说明发生了什么（状态码/断言/错误原因）与下一步建议；不要原样倒出大段 JSON，长响应只讲关键字段。

## 输出风格
- 中文回答（用户界面为中文时），技术名词保留英文（如 JSONPath、gRPC）。
- 先给结论/结果，再给必要的细节；涉及代码/JSON 时用等宽代码块。
- 一次只做用户要求的事；不确定时先用读取类工具确认，或直接问一个关键问题。"#;

const ASK_BLOCK_ZH: &str = r#"## 当前模式：Ask（只读对话）
- 你只能**读**：写工具（create_* / update_*）与执行工具（run_*）在本模式下不可用，调用它们会直接失败。
- 用户提出改动类需求时：讲清**该怎么做**（改哪个接口、加什么脚本/断言），并提示用户切到 Agent 模式执行；若步骤较多，建议先切到 Plan 模式做计划。
- 绝不能出现「已创建」「已修改」「已运行」这类表述——本模式下你没有任何写权限。"#;

const AGENT_BLOCK_ZH: &str = r#"## 当前模式：Agent（可读写）
- 写操作（create_* / update_*）会**立即落库**，界面上会展示变更前后差异；请在完成后简要说明改了哪些对象、在哪个集合/目录下，便于用户复查。
- 执行类工具（run_request / run_scenario / run_load_test）**每次都需要用户确认**：一次只提一个执行请求，并在文字里说明目的与关键参数（如压测的并发与时长）。
- 参数校验失败时会返回可读错误：按提示改正后重试一次；不要用同样的参数反复重试。
- 不要在没有必要时大改已有资产：优先在现有接口/用例上做最小改动。"#;

const PLAN_BLOCK_ZH: &str = r#"## 当前模式：Plan（只读规划）
- 你的产出是**计划**，不是改动：写工具与执行工具在本模式下不可用。
- 流程：先用读取类工具把现状摸清（相关集合/接口/用例的 id、当前定义、环境变量名）→ 需要澄清时先问一个关键问题 → 调用 `present_plan` 提交计划。
- 计划要具体到对象与动作：动哪个接口/用例/套件、改什么字段、用什么参数、如何验证；避免「优化一下」「完善脚本」这类空话。
- 计划可反复修订：用户提意见后再调查、再调用 `present_plan` 覆盖上一版（修订号会递增）。
- 提交后简短说明计划要点，并提示用户点击「开始实现」切到 Agent 模式；**不要**自行开始改动数据，也不要逐字复述整份计划。"#;

const EN_RULES: &str = r#"You are the built-in AI assistant of Orbit (an API testing toolkit). Turn natural language or API descriptions into usable API assets, and drive the project's execution engine for debugging, automation and load testing.

## Principles
1. **Read before write**: always fetch real ids with read tools (list_collections / list_requests / get_request / list_scenarios) before referencing them. Never invent ids.
2. **Only do what is allowed**: your toolbox is determined by the current mode (see "Current mode" below). Capabilities absent from the tool list are not permitted — never pretend you performed them.
3. **Patch, don't rewrite**: update_request / update_scenario take only the fields to change (JSON Merge Patch; null deletes a field). For a **single action** (e.g. the SQL of one DB action, the code of one script) always use the action-level tools update_action / insert_action / delete_action / move_action (`list` + `index`, after reading `get_request`) — resending `preActions` through update_request replaces the whole array and easily drops fields.
4. **Scripts and assertions**: scripts use `pm.*`, assertions use the `assertions` array; **follow the "Syntax reference" section below exactly** — it is the single authority (official comparator names, the full script API list). Do not write them from memory of other tools. Two hard rules: **durations need an explicit unit** (`"2s"` / `"800ms"` — a bare number means seconds, off by 1000×); **a request body needs all three**: `bodyMode` (e.g. `json`), `body` (string) and `contentType` (e.g. `application/json`) — `body` alone is ignored and the request goes out empty.
5. **Variables & dynamic values**: write `{{name}}` or dynamic values `{{$category.method}}` directly in URL / headers / body / scripts; the engine interpolates at run time — never pre-substitute. **Only use the real tokens listed in the Syntax reference**; anything else is sent literally (`{{$uuid.v4}}`, `{{$timestamp.ms}}`, `{{$randomInt}}` do not exist here).
6. **Security**: environment secret values are not visible to you (names only). Never ask the user to paste tokens.
7. **Explain failures**: after a tool result, summarize what happened (status code / assertion / error) and suggest the next step; do not dump large JSON blobs.

## Style
- Answer concisely. Lead with the result, then the necessary details. Use fenced code blocks for JSON/code."#;

const ASK_BLOCK_EN: &str = r#"## Current mode: Ask (read-only)
- You may only **read**: write tools (create_* / update_*) and execution tools (run_*) are unavailable and will fail if called.
- When the user asks for a change, explain **how** it should be done and suggest switching to Agent mode; for multi-step work, suggest Plan mode first.
- Never say something "was created/modified/run" — you have no write access in this mode."#;

const AGENT_BLOCK_EN: &str = r#"## Current mode: Agent (read & write)
- Writes (create_* / update_*) are persisted **immediately**; the UI shows a before/after diff. Summarize what you changed and where.
- Execution tools (run_request / run_scenario / run_load_test) require confirmation **every time**: issue one execution request at a time and state its purpose and key parameters (e.g. load test concurrency and duration).
- Validation failures come back as readable errors: fix them and retry once; never retry with identical arguments.
- Prefer the smallest change to existing assets over rewriting them."#;

const PLAN_BLOCK_EN: &str = r#"## Current mode: Plan (read-only planning)
- Your deliverable is a **plan**, not changes: write and execution tools are unavailable.
- Flow: investigate with read tools (ids, current definitions, variable names) → ask one clarifying question if needed → call `present_plan`.
- Be concrete: which request/scenario/suite, which fields, which parameters, how to verify. Avoid vague items.
- Plans can be revised: investigate again and call `present_plan` to overwrite (the revision number increases).
- After submitting, briefly summarize the plan and tell the user to click "Start implementing" to switch to Agent mode. Never start editing data yourself."#;

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ContextSummary {
        ContextSummary {
            workspace_id: Some("ws-1".into()),
            workspace_name: Some("电商".into()),
            environment_id: Some("env-1".into()),
            environment_name: Some("测试环境".into()),
            variable_names: vec!["baseUrl".into()],
            secret_names: vec!["apiToken".into()],
            collections: vec!["c1 | 用户中心".into()],
            request_count: 12,
            scenario_count: 3,
            suite_count: 1,
            model_names: vec!["LoginResponse".into()],
            data_source_names: vec!["mysql-test".into()],
            selection: Some(Selection {
                kind: "request".into(),
                id: "r1".into(),
                name: "登录".into(),
                detail: "POST https://api.x.com/login".into(),
            }),
            references: Vec::new(),
        }
    }

    #[test]
    fn prompt_contains_rules_and_tool_policy() {
        let p = system_prompt(Language::Zh, &ctx(), AiMode::Agent);
        assert!(p.contains("先读后写"));
        assert!(p.contains("绝不编造 id"));
        assert!(p.contains("pm."));
        assert!(p.contains("{{$"));
    }

    #[test]
    fn prompt_renders_context_but_never_secret_values() {
        let p = system_prompt(Language::Zh, &ctx(), AiMode::Agent);
        assert!(p.contains("测试环境"));
        assert!(p.contains("apiToken"));
        // Only secret names, no values - use a sentinel to verify rendering never leaks the value
        assert!(!p.contains("super-secret-value"));
        assert!(p.contains("用户当前选中的接口"));
        assert!(p.contains("r1"));
    }

    #[test]
    fn english_prompt_used_for_en_locale() {
        let p = system_prompt(Language::En, &ctx(), AiMode::Agent);
        assert!(p.contains("Read before write"));
        assert!(p.contains("Current context"));
    }

    #[test]
    fn language_parsing_defaults_to_chinese() {
        assert_eq!(Language::from_tag("en-US"), Language::En);
        assert_eq!(Language::from_tag("zh-CN"), Language::Zh);
        assert_eq!(Language::from_tag(""), Language::Zh);
    }

    #[test]
    fn empty_context_renders_without_panicking() {
        let p = system_prompt(Language::Zh, &ContextSummary::default(), AiMode::Agent);
        assert!(p.contains("用户当前未选中任何实体"));
        assert!(p.contains("未选择"));
    }

    #[test]
    fn ask_mode_forbids_claiming_changes() {
        let p = system_prompt(Language::Zh, &ctx(), AiMode::Ask);
        assert!(p.contains("Ask（只读对话）"));
        assert!(
            p.contains("已创建"),
            "must explicitly forbid pretending to write"
        );
        assert!(!p.contains("立即落库"));
    }

    #[test]
    fn plan_mode_requires_present_plan_and_forbids_editing() {
        let p = system_prompt(Language::Zh, &ctx(), AiMode::Plan);
        assert!(p.contains("Plan（只读规划）"));
        assert!(p.contains("present_plan"));
        assert!(p.contains("不要") && p.contains("自行开始改动"));
        assert!(!p.contains("立即落库"));
    }

    #[test]
    fn agent_mode_is_the_only_one_that_writes_immediately() {
        let p = system_prompt(Language::Zh, &ctx(), AiMode::Agent);
        assert!(p.contains("Agent（可读写）"));
        assert!(p.contains("立即落库"));
        assert!(
            p.contains("每次都需要用户确认"),
            "execution tools still require confirmation"
        );
        assert!(!p.contains("present_plan"));
    }

    fn refs() -> Vec<Reference> {
        vec![
            Reference {
                kind: "collection".into(),
                id: "c1".into(),
                name: "用户中心".into(),
                detail: String::new(),
                content: None,
            },
            Reference {
                kind: "request".into(),
                id: "r1".into(),
                name: "登录".into(),
                detail: "POST /api/login".into(),
                content: None,
            },
            Reference {
                kind: "file".into(),
                id: "/tmp/petstore.yaml".into(),
                name: "petstore.yaml".into(),
                detail: "2.1 KB".into(),
                content: Some("openapi: 3.0.0\npaths:\n  /pets:\n    get: {}\n".into()),
            },
        ]
    }

    #[test]
    fn references_render_as_explicit_boundary() {
        let p = system_prompt(Language::Zh, &ctx(), AiMode::Agent);
        assert!(
            !p.contains("用户显式引用的对象"),
            "the block must not appear when there are no references"
        );

        let ctx = ContextSummary {
            references: refs(),
            ..ctx()
        };
        let p = system_prompt(Language::Zh, &ctx, AiMode::Agent);
        assert!(p.contains("用户显式引用的对象（本次任务的边界）"));
        assert!(p.contains("只在这个集合内新建/修改接口"));
        assert!(p.contains("接口「登录」(id=r1) POST /api/login"));
        assert!(p.contains("接口定义文件「petstore.yaml」"));
    }

    #[test]
    fn referenced_file_body_is_inlined_verbatim() {
        let ctx = ContextSummary {
            references: refs(),
            ..ctx()
        };
        let p = system_prompt(Language::Zh, &ctx, AiMode::Agent);
        assert!(p.contains("### 引用文件：petstore.yaml"));
        assert!(p.contains("openapi: 3.0.0"));
        assert!(p.contains("get: {}"));
    }

    #[test]
    fn file_reference_without_content_is_reported_as_unreadable() {
        let refs = vec![Reference {
            kind: "file".into(),
            id: "/tmp/x.yaml".into(),
            name: "x.yaml".into(),
            detail: String::new(),
            content: None,
        }];
        let rendered = render_references(Language::Zh, &refs);
        assert!(rendered.contains("文件内容未能读取"));
        assert!(
            !rendered.contains("### 引用文件"),
            "don't emit an empty code block when there's no body"
        );
    }

    #[test]
    fn unknown_reference_kind_still_renders() {
        let refs = vec![Reference {
            kind: "loadTest".into(),
            id: "l1".into(),
            name: "压测目标".into(),
            detail: String::new(),
            content: None,
        }];
        assert!(render_references(Language::Zh, &refs).contains("压测目标"));
    }

    #[test]
    fn empty_references_render_nothing() {
        assert!(render_references(Language::Zh, &[]).is_empty());
        assert!(render_references(Language::En, &[]).is_empty());
    }

    /// The English prompt must not leak the Chinese label set used by [`render_context`] /
    /// [`render_references`] / [`entity_label`]. User-supplied data (workspace and entity names)
    /// is passed through verbatim and is therefore not part of this check.
    #[test]
    fn english_context_and_references_render_in_english() {
        let ctx = ContextSummary {
            references: refs(),
            ..ctx()
        };
        let p = system_prompt(Language::En, &ctx, AiMode::Agent);
        assert!(p.contains("## Objects explicitly referenced by the user (boundary of this task)"));
        assert!(p.contains("create/modify requests only within this collection"));
        assert!(p.contains(
            "treat it as authoritative, do not create another request with the same name"
        ));
        assert!(p.contains("### Referenced file: petstore.yaml"));

        let rendered = render_references(Language::En, &refs());
        for leaked in [
            "用户显式引用的对象",
            "只在这个集合内新建/修改接口",
            "接口定义文件",
            "正文见下方",
            "文件内容未能读取",
            "引用文件",
        ] {
            assert!(
                !rendered.contains(leaked),
                "the English reference block must not contain `{leaked}`: {rendered}"
            );
        }
        assert!(entity_label(Language::En, "loadTest") == "load test target");
        assert!(entity_label(Language::Zh, "loadTest") == "压测目标");

        let empty = system_prompt(Language::En, &ContextSummary::default(), AiMode::Agent);
        assert!(empty.contains("- Active environment: none selected"));
        assert!(empty.contains("- No entity currently selected by the user"));
        assert!(!empty.contains("未选择"));
        assert!(!empty.contains("用户当前未选中任何实体"));
    }

    #[test]
    fn prompt_only_teaches_real_dynamic_tokens() {
        // Regression: the prompt once copied Postman syntax (`{{$uuid.v4}}` / `{{$timestamp.ms}}`),
        // These tokens don't exist in this product's catalog -> AI-generated bodies retain expressions that can never be resolved
        // (a script that sees the expression cannot sign the final request body).
        let zh = system_prompt(Language::Zh, &ctx(), AiMode::Agent);
        let en = system_prompt(Language::En, &ctx(), AiMode::Agent);
        for p in [zh, en] {
            assert!(p.contains("$string.uuid"), "must use a real catalog token");
            assert!(p.contains("$date.timestampMs"));
            // Counter-examples may stay (the model has Postman priors, an explicit "don't write it like this" is more effective),
            // but Postman syntax must never be presented as a recommended example ("such as ...").
            assert!(
                !p.contains("如 `{{$uuid.v4}}`"),
                "must not be a recommended example: {p}"
            );
            assert!(
                !p.contains("such as `{{$uuid.v4}}`"),
                "must not be a recommended example: {p}"
            );
        }
    }

    #[test]
    fn prompt_states_body_trio_and_duration_unit() {
        // Post-mortem: the model supplied only body without bodyMode/contentType, so the request went out empty;
        // and a bare number in an assertion is treated as seconds. Both must be stated explicitly in the prompt.
        let p = system_prompt(Language::Zh, &ctx(), AiMode::Agent);
        assert!(p.contains("bodyMode"), "must name bodyMode explicitly");
        assert!(p.contains("contentType"), "must stress the body trio");
        assert!(
            p.contains("时长必须带单位"),
            "must explain the duration unit"
        );
        let en = system_prompt(Language::En, &ctx(), AiMode::Agent);
        assert!(en.contains("bodyMode") && en.contains("explicit unit"));
    }

    #[test]
    fn mode_block_switches_between_languages() {
        assert!(mode_block(Language::En, AiMode::Plan).contains("Current mode: Plan"));
        assert!(mode_block(Language::En, AiMode::Ask).contains("read-only"));
        assert!(mode_block(Language::Zh, AiMode::Ask).contains("只读对话"));
    }
}
