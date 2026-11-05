// The request pane: open files as tabs, the current request's bar (method, name, resolved URL, copy as curl,
// send), and the `.http` editor with callme's highlighting, completion, hover cards, diagnostics and folds.
import { type Bounds, computed, Overlay, Scroll, signal, Text, View, type ViewRef } from "nsh";
import { Editor, type EditorDecoration, type EditorDiagnostic } from "nsh/base";
import { api, requestLabel, type Hover } from "./api";
import { Icon } from "./icons";
import {
  activeFile,
  closeTab,
  copyCurl,
  curlCommand,
  currentIndex,
  currentRequest,
  currentRun,
  cursorLine,
  editor,
  env,
  goTo,
  naming,
  newRequest,
  openFile,
  send,
  stop,
  setText,
  settledText,
  tabs,
  text,
} from "./store";
import { Button, Divider, IconButton, Method, SendButton } from "./ui";

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
    <View class="h-11 flex-row items-center gap-1 border-b border-border pl-3 pr-2">
      <If when={p() && "url" in p()!} else={<Text class="flex-1 text-xs text-status-err" style={{ numberOfLines: 1 }}>{p() && "message" in p()! ? (p() as { message: string }).message : "光标不在任何请求里"}</Text>}>
        <Method method={(p() as { method: string }).method} class="w-auto text-xs" />
        <Text class="ml-1 text-[13px] font-semibold text-foreground" style={{ numberOfLines: 1 }}>
          {currentRequest() ? requestLabel(currentRequest()!) : `#${currentIndex()}`}
        </Text>
        <Text class="ml-2 flex-1 font-mono text-xs text-muted-foreground" style={{ numberOfLines: 1 }}>{(p() as { url: string }).url}</Text>
      </If>
      <IconButton icon="plus" label="新建请求" shortcut="⌘N" onPress={newRequest} />
      <View ref={(r) => (curlAnchor = r)}>
        <IconButton icon="terminal" label="curl 命令" shortcut="⌘⇧C 直接复制" onPress={() => curlOpen.set(!curlOpen())} />
      </View>
      <CurlCard />
      <Divider />
      <SendButton running={running()} disabled={!currentIndex()} onPress={send} onStop={stop} />
    </View>
  );
}

const curlOpen = signal(false);
let curlAnchor: ViewRef | undefined;

/** The current request as a curl command, to read and copy. */
function CurlCard() {
  const c = () => (curlOpen() ? curlCommand() : null);
  return (
    <If when={curlOpen()}>
      <Overlay
        anchor={curlAnchor}
        placement="bottom"
        align="end"
        offset={6}
        onPressOutside={() => curlOpen.set(false)}
        onCancel={() => curlOpen.set(false)}
        class="w-[560px] rounded-lg border border-border bg-popover"
      >
        <View class="flex-row items-center gap-2 border-b border-border px-3 py-2">
          <Icon name="terminal" size={13} />
          <Text class="flex-1 text-[13px] font-semibold text-foreground">curl</Text>
          <View
            role="button"
            accessibilityLabel="复制 curl 命令"
            onPress={() => void copyCurl().then(() => curlOpen.set(false))}
            class="h-7 flex-row items-center gap-1.5 rounded-md bg-button px-2.5 active:opacity-80"
          >
            <Icon name="copy" size={12} color="button-foreground" />
            <Text class="text-xs font-semibold text-button-foreground">复制</Text>
          </View>
        </View>
        <Scroll class="max-h-[320px] px-3 py-2.5">
          <Text class={`font-mono text-xs ${c() && "error" in c()! ? "text-status-err" : "text-foreground"} select-text`}>
            {c() ? ("text" in c()! ? (c() as { text: string }).text : (c() as { error: string }).error) : ""}
          </Text>
        </Scroll>
        <Text class="border-t border-border px-3 py-1.5 text-[11px] text-muted-foreground">变量已替换成当前环境的值，密钥也是真实值，分享前注意</Text>
      </Overlay>
    </If>
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
              <Text class="text-[11px] text-muted-foreground">⌘ 点击跳转</Text>
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
          onDefinitionRequest={(e) => {
            const d = api.definition(activeFile(), text(), e.position.line, e.position.column, env());
            if (!d) return;
            if (e.trigger === "hover") {
              const line = e.position.line;
              editor.ref?.presentDefinitionLink(e.id, { start: { line, column: d.start }, end: { line, column: d.end } });
            } else {
              hover.set(null);
              goTo(d.file, d.line);
            }
          }}
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
