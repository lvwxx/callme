// The sidebar: the environment files, the project's requests by folder and file, the recent runs.
import { computed, Scroll, signal, Text, View } from "nsh";
import type { RequestRef } from "./api";
import { Icon } from "./icons";
import { activeFile, currentIndex, envTab, files, openFile, recent, requests, saveNow, selectRequest, view } from "./store";
import { Method, SectionLabel, Status } from "./ui";

type Row =
  | { kind: "folder"; key: string; label: string; depth: number; open: boolean }
  | { kind: "file"; key: string; label: string; depth: number; open: boolean; file: string }
  | { kind: "request"; key: string; depth: number; request: RequestRef };

const closed = signal<Set<string>>(new Set());

function toggle(key: string) {
  const next = new Set(closed());
  if (next.has(key)) next.delete(key);
  else next.add(key);
  closed.set(next);
}

/** Folders, files and their requests as indented rows, closed ones collapsed. */
const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  const seen = new Set<string>();
  for (const file of files()) {
    const parts = file.split("/");
    let hidden = false;
    for (let i = 0; i < parts.length - 1; i++) {
      const key = parts.slice(0, i + 1).join("/") + "/";
      if (!seen.has(key)) {
        seen.add(key);
        if (!hidden) out.push({ kind: "folder", key, label: parts[i], depth: i, open: !closed().has(key) });
      }
      hidden ||= closed().has(key);
    }
    if (hidden) continue;
    const depth = parts.length - 1;
    out.push({ kind: "file", key: file, label: parts[parts.length - 1], depth, open: !closed().has(file), file });
    if (closed().has(file)) continue;
    for (const r of requests().filter((r) => r.file === file)) out.push({ kind: "request", key: `${file}#${r.id}`, depth: depth + 1, request: r });
  }
  return out;
});

export function Sidebar() {
  const envFile = (label: string, tab: "public" | "private") => (
    <View
      role="button"
      accessibilityLabel={label}
      onPress={() => (saveNow(), envTab.set(tab), view.set("env"))}
      class={`h-7 flex-row items-center gap-1.5 rounded-md px-2 ${view() === "env" && envTab() === tab ? "bg-list-active" : "hover:bg-list-hover"}`}
    >
      <Icon name={tab === "private" ? "lock" : "list"} />
      <Text class="flex-1 text-[13px] text-foreground" style={{ numberOfLines: 1 }}>{label}</Text>
    </View>
  );
  return (
    <View class="w-[248px] border-r border-border bg-panel">
      <View class="border-b border-border px-1.5 pb-2 pt-2.5">
        <SectionLabel>环境</SectionLabel>
        {envFile("http-client.env.json", "public")}
        {envFile("http-client.private.env.json", "private")}
      </View>
      <View class="h-8 flex-row items-center px-3.5">
        <Text class="flex-1 text-[11px] font-semibold text-muted-foreground">请求</Text>
      </View>
      <Scroll class="flex-1 px-1.5">
        <If when={rows().length === 0}>
          <Text class="px-2 text-xs text-muted-foreground">项目里还没有 .http 文件。用 ⌘K 粘贴一段 curl 来新建一个。</Text>
        </If>
        <Each of={rows()} key={(r: Row) => r.key}>
          {(row) => {
            const r = row();
            const pad = 8 + r.depth * 14 + (r.kind === "folder" ? 0 : 14);
            if (r.kind === "request") {
              const active = () => view() === "request" && activeFile() === r.request.file && currentIndex() === r.request.index;
              return (
                <View
                  role="button"
                  accessibilityLabel={`${r.request.method} ${r.request.name ?? r.request.id}`}
                  onPress={() => selectRequest(r.request)}
                  class={`h-7 flex-row items-center gap-1.5 rounded-md pr-2 ${active() ? "border border-list-active-border bg-list-active" : "hover:bg-list-hover"}`}
                  style={{ paddingLeft: pad }}
                >
                  <Method method={r.request.method} />
                  <Text class="flex-1 text-[13px] text-foreground" style={{ numberOfLines: 1 }}>
                    {r.request.name ?? r.request.title ?? r.request.id}
                  </Text>
                </View>
              );
            }
            return (
              <View
                role="button"
                accessibilityLabel={r.label}
                onPress={() => (r.kind === "file" ? (openFile(r.file), r.open || toggle(r.key)) : toggle(r.key))}
                class="h-7 flex-row items-center gap-1.5 rounded-md pr-2 hover:bg-list-hover"
                style={{ paddingLeft: r.kind === "folder" ? pad : pad - 14 }}
              >
                <View style={{ transform: [{ rotate: r.open ? "90deg" : "0deg" }] }}>
                  <Icon name="chevronRight" size={12} />
                </View>
                <Icon name={r.kind === "folder" ? "folder" : "file"} />
                <Text class="flex-1 text-[13px] text-foreground" style={{ numberOfLines: 1 }}>{r.label}</Text>
              </View>
            );
          }}
        </Each>
      </Scroll>
      <If when={recent().length > 0}>
        <View class="border-t border-border px-1.5 pb-2.5 pt-2">
          <SectionLabel>最近</SectionLabel>
          <Each of={recent()}>
            {(x) => (
              <View class="h-[26px] flex-row items-center gap-2 px-2">
                <Method method={x().method} />
                <Text class="flex-1 text-xs text-foreground" style={{ numberOfLines: 1 }}>{x().id}</Text>
                <Status status={x().status} />
              </View>
            )}
          </Each>
        </View>
      </If>
    </View>
  );
}
