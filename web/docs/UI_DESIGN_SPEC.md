# UI 设计规范（Design Spec）

> 目的：沉淀本项目已确立的 UI 规范，作为后续所有新界面的设计 / 实现标准。
> 覆盖范围：颜色、字号、圆角、间距、组件交互、聚焦态、暗色模式、代码组织。
> 适用范围：`web/` 下的所有前端代码（React + TypeScript + Vite + Tailwind v4 + shadcn/ui）。

---

## 1. 技术栈与基础约定

| 项         | 选型                                                     | 说明                                                            |
| ---------- | -------------------------------------------------------- | --------------------------------------------------------------- |
| 框架       | React 19 + TypeScript + Vite                             | —                                                               |
| 样式       | Tailwind CSS v4（CSS-first，无 `tailwind.config.js`）    | 通过 `@import "tailwindcss"` + `@theme` 在 `src/index.css` 配置 |
| 组件库     | shadcn/ui（neutral 主题，Radix UI 原语）                 | 已落地于 `src/components/ui/*`                                  |
| 变体管理   | `class-variance-authority` (cva)                         | 新组件变体按 `ui/button.tsx` 结构写                             |
| 类名合并   | `cn()` = `clsx` + `tailwind-merge`（`src/lib/utils.ts`） | 条件类 / 冲突消解一律用它                                       |
| 图标       | `lucide-react`                                           | 禁止引入其他图标库                                              |
| 代码编辑器 | CodeMirror 6                                             | 统一走 `codeEditorShared.ts`                                    |
| 路径别名   | `@` → `src`                                              | 见 `vite.config.ts`                                             |

---

## 2. 设计令牌（Design Tokens）

### 2.1 语义颜色（唯一可信来源：`src/index.css` 的 `:root` / `.dark`）

颜色以 `oklch` 定义在 CSS 变量，**组件里只能使用语义类，禁止硬编码十六进制 / oklch / rgb**。

| 令牌（变量）                         | 语义类示例                               | 用途                                                         |
| ------------------------------------ | ---------------------------------------- | ------------------------------------------------------------ |
| `--background` / `--foreground`      | `bg-background` / `text-foreground`      | 页面底色 / 主文字                                            |
| `--card` / `--card-foreground`       | `bg-card`                                | 卡片表面                                                     |
| `--popover`                          | `bg-popover`                             | 浮层 / 下拉 / 弹窗容器                                       |
| `--primary` / `--primary-foreground` | `bg-primary` / `text-primary-foreground` | 主操作、强调（neutral 主题下亮色模式为近黑、暗色模式为近白） |
| `--secondary`                        | `bg-secondary`                           | 次级按钮底                                                   |
| `--muted` / `--muted-foreground`     | `bg-muted` / `text-muted-foreground`     | 弱化表面 / 次要文字、提示、元信息                            |
| `--accent` / `--accent-foreground`   | `bg-accent` / `hover:bg-accent`          | hover / 选中态背景                                           |
| `--destructive`                      | `bg-destructive` / `text-destructive`    | 危险操作、错误、校验失败边框                                 |
| `--success` / `--warning`            | `text-success` / `text-warning`          | 状态码 / 方法徽标（扩展令牌）                                |
| `--border`                           | `border-border`                          | 描边、分割线、卡片边框                                       |
| `--input`                            | `border-input`                           | 输入框默认边框                                               |
| `--ring`                             | `ring-ring`                              | 聚焦发光环颜色                                               |
| `--sidebar-*`                        | `bg-sidebar` 等                          | 侧边栏专用                                                   |

> 扩展色（方法 / 协议 / 状态码）集中在 `src/lib/utils.ts` 的 `methodColor / methodBg / protocolColor / statusColor`，组件**不得本地重复定义**这些颜色。

### 2.2 字体（Typography）

- **字体族**：`--font-sans`（Inter Variable，含 `PingFang SC` / `Microsoft YaHei` 中文回退）用于正文与 UI；`--font-mono`（ui-monospace 等）用于技术内容（代码、URL、Key/Value、JSON）。
- **字号分级（强制语义化，禁止 `text-[Npx]`）**：

  | 层级   | Token       | 像素 | 适用场景                                                   |
  | ------ | ----------- | ---- | ---------------------------------------------------------- |
  | 标题   | `text-lg`   | 18px | 区块 / 弹窗主标题                                          |
  | 大正文 | `text-base` | 16px | 次级主标题；移动断点下输入框字号（`md:text-sm` 切回）      |
  | 副标题 | `text-sm`   | 14px | 分组标题、字段 `<Label>`、列表项主文本、按钮、`<label>`    |
  | 内容   | `text-xs`   | 12px | 元信息、描述、提示、状态码、最小状态文本、技术内容、K-V 值 |

  > 历史：项目曾存在 245 处 `text-[9/10/11px]` 硬编码，已统一为 `text-sm` / `text-xs`。**新代码不得回退到任意 px 写法**。

- 技术内容（代码、URL、key/value、JSON）统一 `font-mono text-xs`。

### 2.3 圆角（Radius）

- 令牌：`--radius: 0.625rem`，并派生 `--radius-sm/md/lg/xl`（`src/index.css`）。
- **控件与容器一律 `rounded-md`**（输入框、按钮、卡片、弹窗内容、下拉浮层）。
- 徽标（Badge）用 `rounded-full`；Checkbox 小方块 `rounded-[4px]`。
- 不要混用随意圆角数值，保持单一圆角语言。

### 2.4 间距与尺寸（Spacing）

- 基于 Tailwind 默认 4px 栅格：`gap-1`(4) / `gap-1.5`(6) / `gap-2`(8) / `gap-3`(12)…
- 表单行：`space-y-1.5`；行内元素：`gap-1.5`；面板内边距：`p-3` / `p-4`。
- 控件高度：默认 `h-9`(36) / 小 `h-8`(32) / 超小 `h-6`(24) / 大 `h-10`(40)。图标按钮用 `size-9` / `icon-sm` `size-8` 等。

---

## 3. 组件与交互规范

### 3.1 聚焦态（Focus Ring）—— 全局统一"边缘发光"

**标准写法**（shadcn neutral 默认，所有可聚焦控件必须遵循）：

```
focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50
```

- **禁止**旧写法 `focus:border-primary/60`（暗色实线边框，已废弃）。
- 参考实现：`ui/input.tsx`、`button.tsx`、`select.tsx`、`textarea.tsx`、`checkbox.tsx`、`tabs.tsx`、`switch.tsx`、`scroll-area.tsx` 等。
- **自定义可聚焦容器**（如 CodeMirror 宿主）用 `focus-within:` 同款：
  ```
  focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50
  ```

**关键实现细节（来自本次修复，必须照做）**：

1. `ring` 是宿主自身的 `box-shadow`，**不受宿主 `overflow-hidden` 裁切** —— 因此宿主可同时拥有 `overflow-hidden` 与聚焦发光环。
2. **圆角裁切交给宿主**：宿主写 `rounded-md overflow-hidden`，内层会绘制不透明背景的子元素（如 CodeMirror 的 `.cm-editor`）**不要再加 `border-radius + overflow:hidden`**。否则内层不透明背景会盖住宿主 1px 边框，导致圆角处边框缺失 / 变淡（已踩坑）。
3. **关闭内部可聚焦元素的浏览器默认 outline**（如 CodeMirror 的 `.cm-content` / `.cm-scroller` 设 `outline: none`），否则聚焦时会出现方形黑框，与圆角发光环冲突。

### 3.2 表单与 K-V 编辑器范式

- 优先复用 `ui/input.tsx`、`ui/checkbox.tsx`、`ui/button.tsx`、`ui/label.tsx`。
- Key/Value 输入用 `font-mono text-xs`（技术内容）。
- 字段标签用 `<Label>`（`ui/label.tsx`），默认 `text-sm`，强调可加 `font-semibold` / `uppercase`。
- 参考实现：`src/components/common/KeyValueEditor.tsx`（含下拉建议浮层、动态值插入、空态 `EmptyHint`）。
- 浮层 / 下拉容器统一：
  ```
  rounded-md border border-border bg-popover p-1 shadow-md
  ```
- 空态：`icon + text-xs text-muted-foreground`（参考 `EmptyHint`）。

### 3.3 按钮（Button）

- 变体：`default` / `destructive` / `outline` / `secondary` / `ghost` / `link`（见 `ui/button.tsx`）。
- 尺寸：`default h-9` / `sm h-8` / `xs h-6` / `lg h-10` / `icon*`。
- 图标按钮用 `size="icon-sm"` 等；svg 默认 `size-4`，小号 `size-3.5`（已有 `[&_svg]` 规则，不要手写尺寸类除非带 `size-` 前缀）。
- 删除 / 危险操作使用 `variant="destructive"`；次要动作使用 `variant="ghost"` / `outline`。

### 3.4 徽标 / 状态色

- HTTP 方法、协议、状态码颜色全部走 `src/lib/utils.ts` 的 `methodColor / methodBg / protocolColor / statusColor`，**组件不得本地重复定义**。
- 方法徽标风格：`bg-*/15 text-*/300 border-*/30`（半透明底 + 亮字 + 描边）。
- 通用徽标用 `ui/badge.tsx`（`rounded-full text-xs`）。

### 3.5 弹窗 / 浮层 / 选中高亮

- 弹窗 / 浮层 / 菜单一律用 shadcn + Radix 原语：`Dialog` / `Popover` / `DropdownMenu` / `Tooltip`。
- 列表 / 行 hover：`hover:bg-accent/15`；选中 / active：`bg-accent` 或数据驱动的 `data-[state=active]`（参考 Tabs）。
- **"左侧列表 + 右侧编辑"布局（如环境变量弹窗）**：
  - 左侧项**必须有清晰选中态**（`bg-accent` + 文字加亮，或左侧指示条）。
  - 右侧编辑区顶部**显示当前编辑对象名**，避免用户不知道在编辑哪一项。

### 3.6 滚动条（Scrollbar）

- 全局细滚动条已在 `src/index.css` 统一：`scrollbar-width: thin`，webkit 宽 8px、thumb `var(--border)`、圆角 4px。
- 横向 TabBar 用 `.tab-scroll` 彻底隐藏滚动条（不挤压标签高度）。
- 自定义滚动区优先用 `ui/scroll-area.tsx`。

---

## 4. 暗色模式

- 由 `.dark` 类（挂在根节点）切换；所有语义令牌自动映射，**组件无需写 `dark:` 前缀即可适配**。
- 仅少数控件需调暗时补 `dark:bg-input/30` / `dark:hover:bg-input/50`（参考 `input.tsx` / `textarea.tsx`）。
- **禁止**为颜色写两套硬编码（亮 / 暗各一份 hex）。

---

## 5. 代码组织要求

- `className` 一律经 `cn(...)` 合并。
- 新增组件优先复用 `src/components/ui/*`（shadcn 全家桶已齐备：avatar / badge / button / card / checkbox / collapsible / dialog / dropdown-menu / input / label / popover / progress / scroll-area / select / separator / skeleton / switch / table / tabs / textarea / tooltip）。
- 需新组件时从 shadcn 生成，遵循其 `cva` 变体结构（见 `ui/button.tsx` 顶部注释）。
- **不要**私自引入新 UI 库或新设计语言。
- CodeMirror 编辑器统一走 `codeEditorShared.ts` 主题 + `BodyEditor` / `ScriptEditor` 宿主结构，保持聚焦 / 圆角一致。
- 分包：CodeMirror 已在 `vite.config.ts` 用 `manualChunks` 单独拆包；新增重依赖按需追加。

---

## 6. PR 前自检清单

- [ ] 字号仅用 `text-lg / text-base / text-sm / text-xs`，无 `text-[Npx]` 回退
- [ ] 颜色仅用语义令牌类（如 `text-muted-foreground` / `bg-popover`），无裸 `hex` / `oklch` / `rgb`
- [ ] 所有可聚焦元素具备 `focus-visible:ring-[3px] ring-ring/50`（或自定义容器 `focus-within:` 同款）
- [ ] 圆角统一 `rounded-md`（徽标 `rounded-full`）；宿主负责 `overflow-hidden` 裁切内层
- [ ] 复用了 `ui/*` 组件，未私自引入新库
- [ ] 图标来自 `lucide-react`，尺寸走 `size-3.5 / size-4` 与既有 `[&_svg]` 规则
- [ ] 暗色模式无需硬编码即适配
- [ ] 列表 / 行具备 hover 与选中 / active 态；"左列表 + 右编辑"布局已标明当前编辑对象

```

```
