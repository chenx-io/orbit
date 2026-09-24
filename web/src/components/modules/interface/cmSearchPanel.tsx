/* eslint-disable react/only-export-components -- 面板工厂函数与组件强相关，无 fast-refresh 边界需求 */
// CodeMirror 查找/替换面板的自定义实现（VS Code 风格）。
// 通过 search({ createPanel }) 注入，替代 @codemirror/search 的内置面板：
// - React + lucide 图标 + Tailwind 渲染，样式直接对齐 UI_DESIGN_SPEC（语义令牌类）；
// - 大小写 / 正则 / 整词 为图标开关，内嵌在查找输入框右侧；
// - 上一个/下一个/全部/替换/全部替换 为图标按钮（IDE 惯例）；
// - 文案随应用语言（useT/locale）切换；
// - 交互与内置面板一致：输入实时更新 query，Enter / Shift+Enter 上下跳转，
//   Esc 关闭，Mod-f / Mod-r 在面板内聚焦查找 / 替换输入框。
import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CaseSensitive,
  ChevronDown,
  ChevronUp,
  Regex,
  Replace,
  ReplaceAll,
  TextSelect,
  WholeWord,
  X,
} from "lucide-react";
import {
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  replaceAll,
  replaceNext,
  SearchQuery,
  selectMatches,
  setSearchQuery,
} from "@codemirror/search";
import type { EditorView, Panel } from "@codemirror/view";
import type { Locale } from "@/data/types";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

// 面板文案（自定义面板不走 CM phrase 体系，直接按语言取值）
const LABELS: Record<Locale, Record<LabelKey, string>> = {
  "zh-CN": {
    find: "查找",
    replace: "替换为",
    next: "下一个（Enter）",
    prev: "上一个（Shift+Enter）",
    all: "选中全部匹配",
    doReplace: "替换",
    doReplaceAll: "全部替换",
    close: "关闭（Esc）",
    caseSensitive: "区分大小写",
    regexp: "正则表达式",
    wholeWord: "整词匹配",
  },
  "en-US": {
    find: "Find",
    replace: "Replace with",
    next: "Next (Enter)",
    prev: "Previous (Shift+Enter)",
    all: "Select all matches",
    doReplace: "Replace",
    doReplaceAll: "Replace all",
    close: "Close (Esc)",
    caseSensitive: "Match case",
    regexp: "Use regular expression",
    wholeWord: "Match whole word",
  },
};
type LabelKey =
  | "find"
  | "replace"
  | "next"
  | "prev"
  | "all"
  | "doReplace"
  | "doReplaceAll"
  | "close"
  | "caseSensitive"
  | "regexp"
  | "wholeWord";

// 输入框样式：与 ui/input.tsx 同款语义类（rounded-md / border-input / focus ring）
const inputCls =
  "h-7 rounded-md border border-input bg-background px-2 font-mono text-xs text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50";

function IconBtn({
  title,
  onClick,
  className,
  children,
}: {
  title: string;
  onClick: () => void;
  className?: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      // 阻止按钮抢焦点，保持输入框焦点（VS Code 行为）
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className={cn(
        "inline-flex size-6 shrink-0 items-center justify-center rounded-md text-foreground/80 transition-colors",
        "hover:bg-accent hover:text-accent-foreground",
        "focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
        className,
      )}
    >
      {children}
    </button>
  );
}

function ToggleIcon({
  title,
  active,
  onClick,
  children,
}: {
  title: string;
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      aria-pressed={active}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className={cn(
        "inline-flex size-5 items-center justify-center rounded-[4px] transition-colors",
        "focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
        active
          ? "bg-primary/10 text-primary"
          : "text-muted-foreground hover:bg-accent hover:text-accent-foreground",
      )}
    >
      {children}
    </button>
  );
}

function SearchPanelBody({ view }: { view: EditorView }) {
  const { locale } = useT();
  const L = LABELS[locale];

  // 初始 query：来自 editor search state（打开时若选中了文本会自动带入）
  const init = getSearchQuery(view.state);
  const [search, setSearch] = useState(init.search);
  const [replaceText, setReplaceText] = useState(init.replace);
  const [caseSensitive, setCaseSensitive] = useState(init.caseSensitive);
  const [regexp, setRegexp] = useState(init.regexp);
  const [wholeWord, setWholeWord] = useState(init.wholeWord);
  const readOnly = view.state.readOnly;

  const searchRef = useRef<HTMLInputElement>(null);
  const replaceRef = useRef<HTMLInputElement>(null);

  // 输入即更新 query（与内置面板一致；跳转由 Enter / 按钮触发）
  useEffect(() => {
    view.dispatch({
      effects: setSearchQuery.of(
        new SearchQuery({
          search,
          replace: replaceText,
          caseSensitive,
          regexp,
          wholeWord,
        }),
      ),
    });
  }, [view, search, replaceText, caseSensitive, regexp, wholeWord]);

  // 打开时聚焦查找输入框并全选（与内置面板一致）
  useEffect(() => {
    searchRef.current?.focus();
    searchRef.current?.select();
  }, []);

  const onKeyDown = (e: ReactKeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    if (e.key === "Enter" && !mod && !e.altKey) {
      e.preventDefault();
      (e.shiftKey ? findPrevious : findNext)(view);
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeSearchPanel(view);
    } else if (mod && e.key.toLowerCase() === "f") {
      e.preventDefault();
      searchRef.current?.focus();
      searchRef.current?.select();
    } else if (mod && e.key.toLowerCase() === "r" && !readOnly) {
      e.preventDefault();
      replaceRef.current?.focus();
      replaceRef.current?.select();
    }
  };

  return (
    <div
      onKeyDown={onKeyDown}
      className="flex flex-col gap-1.5 rounded-md border border-border bg-popover p-1.5 font-sans text-xs text-popover-foreground shadow-lg"
    >
      {/* 查找行：输入框（右侧内嵌开关）+ 跳转/全选/关闭 */}
      <div className="flex items-center gap-1">
        <div className="relative">
          <input
            ref={searchRef}
            name="search"
            /* openSearchPanel 依赖 [main-field] 聚焦查找输入框 */
            main-field="true"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={L.find}
            spellCheck={false}
            className={cn(inputCls, "w-56 pr-[68px]")}
          />
          <div className="absolute inset-y-0 right-0.5 flex items-center gap-0.5">
            <ToggleIcon
              title={L.caseSensitive}
              active={caseSensitive}
              onClick={() => setCaseSensitive((v) => !v)}
            >
              <CaseSensitive className="size-3.5" />
            </ToggleIcon>
            <ToggleIcon
              title={L.regexp}
              active={regexp}
              onClick={() => setRegexp((v) => !v)}
            >
              <Regex className="size-3.5" />
            </ToggleIcon>
            <ToggleIcon
              title={L.wholeWord}
              active={wholeWord}
              onClick={() => setWholeWord((v) => !v)}
            >
              <WholeWord className="size-3.5" />
            </ToggleIcon>
          </div>
        </div>
        <IconBtn title={L.prev} onClick={() => findPrevious(view)}>
          <ChevronUp className="size-4" />
        </IconBtn>
        <IconBtn title={L.next} onClick={() => findNext(view)}>
          <ChevronDown className="size-4" />
        </IconBtn>
        <IconBtn title={L.all} onClick={() => selectMatches(view)}>
          <TextSelect className="size-4" />
        </IconBtn>
        <IconBtn
          title={L.close}
          onClick={() => closeSearchPanel(view)}
          className="ml-auto text-muted-foreground"
        >
          <X className="size-4" />
        </IconBtn>
      </div>

      {/* 替换行（只读编辑器隐藏） */}
      {!readOnly && (
        <div className="flex items-center gap-1">
          <input
            ref={replaceRef}
            name="replace"
            value={replaceText}
            onChange={(e) => setReplaceText(e.target.value)}
            placeholder={L.replace}
            spellCheck={false}
            className={cn(inputCls, "w-56")}
          />
          <IconBtn title={L.doReplace} onClick={() => replaceNext(view)}>
            <Replace className="size-4" />
          </IconBtn>
          <IconBtn title={L.doReplaceAll} onClick={() => replaceAll(view)}>
            <ReplaceAll className="size-4" />
          </IconBtn>
        </div>
      )}
    </div>
  );
}

/**
 * 创建自定义查找面板（供 search({ createPanel }) 使用）。
 * 用 React root 渲染面板内容；面板销毁时卸载 root。
 */
export function createSearchPanel(view: EditorView): Panel {
  const doc = view.dom.ownerDocument;
  const dom = doc.createElement("div");
  const root: Root = createRoot(dom);
  root.render(<SearchPanelBody view={view} />);
  return {
    dom,
    top: true,
    destroy() {
      root.unmount();
    },
  };
}
