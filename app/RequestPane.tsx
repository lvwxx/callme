// The request pane: open files as tabs, the current request's bar (method, name, resolved URL, copy as curl,
// send), and the `.http` editor with callme's highlighting, completion, hover cards, diagnostics and folds.
import { type Bounds, computed, Overlay, signal, Text, View } from "nsh";
import { Editor, type EditorDecoration, type EditorDiagnostic } from "nsh/base";
import { api, type Hover } from "./api";
import { Icon } from "./icons";
import {
  activeFile,
  closeTab,
  copyCurl,
  currentIndex,
  currentRequest,
  currentRun,
  cursorLine,
  editor,
  env,
  naming,
  newRequest,
  openFile,
  send,
  setText,
  settledText,
  tabs,
  text,
} from "./store";
import { Button } from "./ui";

const hover = signal<{ data: Hover | null; anchor: Bounds; messages: string[] } | null>(null);

const diagnostics = computed(() => (activeFile() ? api.diagnostics(activeFile(), settledText(), env()) : []));

const editorDiagnostics = computed<EditorDiagnostic[]>(() =>
  diagnostics().map((d) => ({
    range: { start: { line: d.line, column: d.start }, end: { line: d.line, column: d.end } },
    severity: d.severity,
    message: d.message,
  })),
);

const folds = computed(() => api.httpFolds(text()));

/** Variables as chips, and the current request marked in the gutter. */
const decorations = computed<EditorDecoration[]>(() => {
  const out: EditorDecoration[] = api.variables(text()).map((v) => ({
    range: { start: { line: v.line, column: v.start }, end: { line: v.line, column: v.end } },
    class: "bg-variable-bg text-variable rounded",
  }));
  const f = folds().find((f) => f.startLine <= cursorLine() && cursorLine() <= f.endLine);
  const lines = text().split("\n");
  const block = f ?? (lines.length ? { startLine: cursorLine(), endLine: cursorLine() } : null);
  if (block && currentIndex()) {
    out.push({
      range: { start: { line: block.startLine, column: 0 }, end: { line: block.endLine, column: lines[block.endLine]?.length ?? 0 } },
      gutter: { class: "bg-brand" },
    });
  }
  return out;
});

const preview = computed(() => (activeFile() && currentIndex() ? api.preview(activeFile(), settledText(), currentIndex(), env()) : null));

function RequestBar() {
  const p = () => preview();
  const running = () => currentRun()?.running ?? false;
  return (
    <View class="h-11 flex-row items-center gap-2 border-b border-border px-3">
      <If when={p() && "url" in p()!} else={<Text class="flex-1 text-xs text-status-err" style={{ numberOfLines: 1 }}>{p() && "message" in p()! ? (p() as { message: string }).message : "光标不在任何请求里"}</Text>}>
        <Text class="font-mono text-xs font-bold text-method-get">{(p() as { method: string }).method}</Text>
        <Text class="text-[13px] font-semibold text-foreground">{currentRequest()?.name ?? `#${currentIndex()}`}</Text>
        <Text class="flex-1 font-mono text-xs text-muted-foreground" style={{ numberOfLines: 1 }}>{(p() as { url: string }).url}</Text>
      </If>
      <Button label="新建请求" hint="⌘N" onPress={newRequest} />
      <Button label="复制为 curl" onPress={copyCurl} />
      <Button label={running() ? "发送中" : "发送"} hint="⌘↵" primary disabled={running() || !currentIndex()} onPress={send} />
    </View>
  );
}

function FileTabs() {
  return (
    <View class="h-9 flex-row border-b border-border bg-tab-bar" role="tablist">
      <Each of={tabs()}>
        {(file) => {
          const on = () => file() === activeFile();
          const parts = file().split("/");
          return (
            <View
              role="tab"
              accessibilityState={{ selected: on() }}
              onPress={() => openFile(file())}
              class={`flex-row items-center gap-2 border-r border-border px-3.5 ${on() ? "border-t-2 border-t-brand bg-tab-active" : ""}`}
            >
              <Text class={`text-[13px] ${on() ? "text-foreground" : "text-muted-foreground"}`}>{parts[parts.length - 1]}</Text>
              <If when={parts.length > 1}>
                <Text class="text-xs text-muted-foreground">{parts.slice(0, -1).join("/") + "/"}</Text>
              </If>
              <View role="button" accessibilityLabel={`关闭 ${file()}`} onPress={() => closeTab(file())} class="rounded p-0.5 hover:bg-list-hover">
                <Icon name="x" size={11} />
              </View>
            </View>
          );
        }}
      </Each>
    </View>
  );
}

function HoverCard() {
  return (
    <If when={hover()}>
      {(h) => (
        <Overlay anchor={h().anchor} placement="bottom" align="start" offset={4} class="w-[420px] rounded-lg border border-border bg-popover">
          <If when={h().data}>
          {(d) => (
          <>
          <View class="flex-row items-center gap-2 px-3 pb-2 pt-2.5">
            <View class="rounded bg-variable-bg px-1">
              <Text class="font-mono text-[13px] font-semibold text-variable">{d().name}</Text>
            </View>
            <Text class="text-[11px] text-muted-foreground">
              {d().kind === "variable" ? "环境变量" : d().kind === "response" ? "响应引用" : "动态变量"}
            </Text>
          </View>
          <If when={d().kind === "variable"}>
            <View class="px-1.5 pb-1.5">
              <Each of={d().values}>
                {(v) => (
                  <View class={`h-7 flex-row items-center gap-2.5 rounded-md px-1.5 ${v().current ? "bg-list-active" : ""}`}>
                    <Text class={`w-16 text-[13px] ${v().current ? "font-semibold" : ""} text-foreground`}>{v().env}</Text>
                    <Text class={`flex-1 font-mono text-xs ${v().value ? "text-foreground" : "text-warning"}`} style={{ numberOfLines: 1 }}>
                      {v().value ?? "未定义"}
                    </Text>
                    <Text class="text-[11px] text-muted-foreground">{v().current ? "当前" : ""}</Text>
                  </View>
                )}
              </Each>
            </View>
          </If>
          <If when={d().kind !== "variable"}>
            <View class="gap-1 px-3 pb-2.5">
              <If when={d().value}>
                <Text class="font-mono text-xs text-foreground" style={{ numberOfLines: 3 }}>{d().value!}</Text>
              </If>
              <Text class="text-xs text-muted-foreground">{d().note ?? ""}</Text>
            </View>
          </If>
          <If when={d().definedIn}>
            <View class="flex-row justify-between border-t border-border px-3 py-1.5">
              <Text class="font-mono text-[11px] text-muted-foreground">{d().definedIn!}</Text>
            </View>
          </If>
          </>
          )}
          </If>
          <If when={h().messages.length > 0}>
            <View class="px-3 py-2">
              <Each of={h().messages}>{(m) => <Text class="text-xs text-warning">{m()}</Text>}</Each>
            </View>
          </If>
        </Overlay>
      )}
    </If>
  );
}

export function RequestPane() {
  return (
    <View class="min-w-0 flex-1 bg-background">
      <FileTabs />
      <If when={activeFile()} else={<Empty />}>
        <RequestBar />
        <Editor
          ref={(r) => (editor.ref = r)}
          value={text()}
          onChange={({ value }) => setText(value)}
          onSelectionChange={({ selections }) => {
            const s = selections[selections.length - 1];
            if (s) cursorLine.set(s.head.line);
          }}
          testID="request-editor"
          language="http"
          completion
          completionTriggerCharacters={["{{", ": ", "@"]}
          onCompletionRequest={(e) => {
            const items = api.completions(activeFile(), text(), e.position.line, e.position.column, env());
            editor.ref?.presentCompletions(e.id, items as never);
          }}
          onHover={(e) => {
            const data = api.hover(activeFile(), text(), e.position.line, e.position.column, env());
            const messages = e.diagnostics.map((d) => d.message);
            hover.set(data || messages.length ? { data, anchor: e.pageBounds, messages } : null);
          }}
          onHoverEnd={() => hover.set(null)}
          diagnostics={editorDiagnostics()}
          decorations={decorations()}
          foldRanges={folds()}
          class="flex-1"
        />
        <HoverCard />
      </If>
    </View>
  );
}

function Empty() {
  return (
    <View class="flex-1 items-center justify-center gap-2">
      <Text class="text-sm text-muted-foreground">在左侧选择一个请求</Text>
      <Button label="新建请求文件" hint="⌘⇧N" onPress={() => naming.set({ kind: "create" })} />
      <Text class="text-xs text-muted-foreground">或者按 ⌘K 粘贴一段 curl</Text>
    </View>
  );
}
