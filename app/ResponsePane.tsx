// The response pane: status, time and size; the body (JSON folded by its structure, filtered by JSONPath),
// server-sent events as they arrive, headers, the time of each phase, and the request's history.
import { computed, Scroll, signal, Text, View } from "nsh";
import { Editor, Input } from "nsh/base";
import { api, type Run } from "./api";
import { Icon } from "./icons";
import { Button, IconButton, Status, Tabs, Tip, size } from "./ui";
import { currentRequest, currentRun, editor, missingDependency, notice, runs, sendAfter } from "./store";

type Tab = "body" | "headers" | "timing" | "history";
const tab = signal<Tab>("body");
const query = signal("");
/** A history record picked in the history tab, shown in place of the newest run. */
const picked = signal<Run | null>(null);

const shown = computed<Run | null>(() => picked() ?? currentRun()?.run ?? null);

/** A record's time in the local time zone: `2026-11-04 18:18:49`. */
function localTime(at: string): string {
  const d = new Date(at);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function bodyText(run: Run | null): string {
  if (!run) return "";
  const b = run.response.body;
  if (b === null || b === undefined) return "";
  return typeof b === "string" ? b : JSON.stringify(b, null, 2);
}

const filtered = computed<{ text: string; error?: string }>(() => {
  const run = shown();
  const text = bodyText(run);
  const q = query().trim();
  if (!q || !run || run.response.bodyKind !== "json") return { text };
  try {
    const out = api.filter(JSON.stringify(run.response.body), q.startsWith("$") ? q : `$.${q}`);
    return { text: JSON.stringify(out.length === 1 ? out[0] : out, null, 2) };
  } catch (e) {
    return { text, error: String(e instanceof Error ? e.message : e) };
  }
});

const headerCount = (h: Run["response"]["headers"]) => Object.values(h).reduce((n, v) => n + (Array.isArray(v) ? v.length : 1), 0);

function Summary() {
  const state = () => currentRun();
  return (
    <View class="h-11 flex-row items-center gap-3 border-b border-border px-3">
      <If
        when={shown()}
        else={
          <>
            <Text class={`flex-1 text-[13px] ${state()?.error ? "text-status-err" : "text-muted-foreground"}`} style={{ numberOfLines: 2 }}>
              {state()?.error ? [state()!.error!.message, state()!.error!.hint].filter(Boolean).join("\n") : state()?.status ?? "按 ⌘↵ 发送当前请求"}
            </Text>
            <If when={missingDependency(state()?.error?.message)}>
              {(name) => <Button label={`先运行 ${name()}，再运行这个请求`} onPress={() => void sendAfter(name())} />}
            </If>
          </>
        }
      >
        {(r) => (
          <>
            <Status status={r().response.status} />
            <Text class="font-mono text-xs text-muted-foreground">{r().response.reason}</Text>
            <Text class="font-mono text-xs text-foreground">{`${r().timing.total} ms`}</Text>
            <Text class="font-mono text-xs text-foreground">{size(r().response.size)}</Text>
            <Text class="flex-1 text-right text-xs text-muted-foreground" style={{ numberOfLines: 1 }}>
              {picked() ? `历史 · ${localTime(r().at).slice(11)}` : state()?.status ?? ""}
            </Text>
            <IconButton
              icon="copy"
              label="复制响应体"
              onPress={() => void api.copy(filtered().text).then(() => notice.set("已复制响应体"))}
            />
          </>
        )}
      </If>
    </View>
  );
}

function Body() {
  const run = () => shown();
  const events = () => currentRun()?.events ?? [];
  const streaming = () => (currentRun()?.running ?? false) && events().length > 0;
  const sse = () => !picked() && (streaming() || run()?.response.bodyKind === "sse");
  return (
    <>
      <View class="h-11 flex-row items-center gap-2 border-b border-border pl-3 pr-2">
        <View class="h-7 flex-1 flex-row items-center gap-2 rounded-md bg-muted/40 px-2">
          <Icon name="filter" size={13} />
          <Input
            value={query()}
            onChange={({ value }) => query.set(value)}
            placeholder="JSONPath，如 $.user.roles"
            class="flex-1 font-mono text-xs text-foreground placeholder:text-muted-foreground"
          />
        </View>
        <View class="flex-row gap-0.5 rounded-md bg-tab-bar p-0.5" role="group" accessibilityLabel="折叠">
          <FoldIcon icon="foldAll" label="全部折叠" onPress={() => (level.set(0), editor.response?.run("foldAll"))} />
          <Each of={[1, 2, 3]}>
            {(n) => (
              <View
                role="button"
                accessibilityLabel={`展开到第 ${n()} 层`}
                tooltip={<Tip label={`展开到第 ${n()} 层`} />}
                onPress={() => (level.set(n()), editor.response?.run("unfoldAll"), editor.response?.foldLevel(n() + 1))}
                class={`h-6 justify-center rounded px-2 ${level() === n() ? "bg-tab-active" : "hover:bg-list-hover"}`}
              >
                <Text class={`text-xs ${level() === n() ? "font-semibold text-foreground" : "text-muted-foreground"}`}>{String(n())}</Text>
              </View>
            )}
          </Each>
          <FoldIcon icon="unfoldAll" label="全部展开" onPress={() => (level.set(null), editor.response?.run("unfoldAll"))} />
        </View>
      </View>
      <If when={filtered().error}>
        <Text class="px-3 py-1 text-xs text-status-err">{filtered().error!}</Text>
      </If>
      <If
        when={sse()}
        else={
          <Editor
            ref={(r) => (editor.response = r)}
            value={filtered().text}
            language={run()?.response.bodyKind === "json" ? "json" : "text"}
            readOnly
            foldRanges={api.jsonFolds(filtered().text)}
            class="flex-1"
          />
        }
      >
        <Scroll class="flex-1">
          <Each of={streaming() || !run() ? events() : (run()!.response.body as unknown[])}>
            {(e) => {
              const ev = e() as { event?: string; data?: unknown };
              return (
                <View class="flex-row gap-3 border-b border-row-border px-3 py-1.5">
                  <Text class="w-16 font-mono text-xs text-variable">{ev.event ?? "message"}</Text>
                  <Text class="flex-1 font-mono text-xs text-foreground">{typeof ev.data === "string" ? ev.data : JSON.stringify(ev.data)}</Text>
                </View>
              );
            }}
          </Each>
        </Scroll>
      </If>
    </>
  );
}

/** The level the body was folded to by the toolbar: 0 all, 1–3 a level, null none. */
const level = signal<number | null>(null);

function FoldIcon(props: { icon: "foldAll" | "unfoldAll"; label: string; onPress: () => void }) {
  return (
    <View
      role="button"
      accessibilityLabel={props.label}
      tooltip={<Tip label={props.label} />}
      onPress={props.onPress}
      class="h-6 w-[26px] items-center justify-center rounded hover:bg-list-hover"
    >
      <Icon name={props.icon} size={14} />
    </View>
  );
}

function HeadersTab() {
  const rows = () => {
    const h = shown()?.response.headers ?? {};
    return Object.entries(h).flatMap(([k, v]) => (Array.isArray(v) ? v : [v]).map((x) => [k, x] as const));
  };
  return (
    <Scroll class="flex-1">
      <Each of={rows()}>
        {(r) => (
          <View class="flex-row gap-3 border-b border-row-border px-3 py-1.5">
            <Text class="w-[150px] font-mono text-xs text-muted-foreground">{r()[0]}</Text>
            <Text class="flex-1 font-mono text-xs text-foreground">{r()[1]}</Text>
          </View>
        )}
      </Each>
    </Scroll>
  );
}

function TimingTab() {
  const phases = () => {
    const t = shown()?.timing;
    if (!t) return [];
    const list = [
      ["DNS", t.dns],
      ["建立连接", t.connect],
      ["TLS 握手", t.tls],
      ["等待首字节", t.ttfb],
      ["下载", t.download],
    ] as const;
    let start = 0;
    const total = t.total || 1;
    return list.map(([label, ms]) => {
      const row = { label, ms, left: (start / total) * 100, width: Math.max((ms / total) * 100, 1) };
      start += ms;
      return row;
    });
  };
  return (
    <View class="flex-1 gap-2.5 px-3 py-4">
      <Each of={phases()}>
        {(p) => (
          <View class="flex-row items-center gap-3">
            <Text class="w-[72px] text-xs text-muted-foreground">{p().label}</Text>
            <View class="h-2.5 flex-1 rounded-sm bg-muted">
              <View class="absolute bottom-0 top-0 rounded-sm bg-brand" style={{ left: `${p().left}%`, width: `${p().width}%` }} />
            </View>
            <Text class="w-[60px] text-right font-mono text-xs text-foreground">{`${p().ms} ms`}</Text>
          </View>
        )}
      </Each>
      <If when={shown()}>
        {(r) => (
          <View class="mt-1 flex-row justify-between border-t border-border pt-2.5">
            <Text class="text-xs text-muted-foreground">{`总计 · ${r().response.httpVersion}`}</Text>
            <Text class="font-mono text-xs font-bold text-foreground">{`${r().timing.total} ms`}</Text>
          </View>
        )}
      </If>
    </View>
  );
}

function HistoryTab() {
  const items = computed(() => {
    void runs();
    const r = currentRequest();
    return r ? api.history(r.file, r.id, 30) : [];
  });
  return (
    <Scroll class="flex-1">
      <If when={items().length === 0}>
        <Text class="px-3 py-3 text-xs text-muted-foreground">还没有运行记录</Text>
      </If>
      <Each of={items()}>
        {(h) => (
          <View
            role="button"
            onPress={() => (picked.set(h()), tab.set("body"))}
            class="flex-row items-center gap-3 border-b border-row-border px-3 py-2 hover:bg-list-hover"
          >
            <Status status={h().response.status} />
            <Text class="w-[64px] font-mono text-xs text-foreground">{`${h().timing.total} ms`}</Text>
            <Text class="flex-1 font-mono text-xs text-muted-foreground">{h().request.env ?? "-"}</Text>
            <Text class="font-mono text-xs text-muted-foreground">{localTime(h().at)}</Text>
          </View>
        )}
      </Each>
    </Scroll>
  );
}

export function ResponsePane() {
  const tabs = () => [
    { id: "body" as Tab, label: "响应体" },
    { id: "headers" as Tab, label: shown() ? `响应头 · ${headerCount(shown()!.response.headers)}` : "响应头" },
    { id: "timing" as Tab, label: "耗时" },
    { id: "history" as Tab, label: "历史" },
  ];
  return (
    <View class="flex-1 bg-panel">
      <Summary />
      <Tabs tabs={tabs()} value={tab()} onChange={(t) => tab.set(t)} />
      <If when={tab() === "body"}>
        <Body />
      </If>
      <If when={tab() === "headers"}>
        <HeadersTab />
      </If>
      <If when={tab() === "timing"}>
        <TimingTab />
      </If>
      <If when={tab() === "history"}>
        <HistoryTab />
      </If>
    </View>
  );
}

/** Forgets a picked history record (a new run, another request). */
export function clearPicked() {
  picked.set(null);
}
