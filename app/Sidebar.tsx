// The sidebar: the environment files, the project's requests by folder and file, the recent runs.
import { computed, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import { Menu } from "nsh/desktop";
import { api, requestLabel } from "./api";
import type { RequestRef } from "./api";
import { Icon } from "./icons";
import {
  activeFile,
  createFile,
  currentIndex,
  envTab,
  files,
  naming,
  openFile,
  recent,
  renameFile,
  renameRequest,
  setTitle,
  copyCurl,
  afterNaming,
  requests,
  saveNow,
  selectRequest,
  trashFile,
  view,
} from "./store";
import { IconButton, Method, SectionLabel, Status } from "./ui";

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
    // The key holds what a row shows: a request retitled is drawn again.
    for (const r of requests().filter((r) => r.file === file))
      out.push({ kind: "request", key: `${file}#${r.id}#${r.method}#${r.title ?? ""}`, depth: depth + 1, request: r });
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
      <View class="h-8 flex-row items-center pl-3.5 pr-1.5">
        <Text class="flex-1 text-[11px] font-semibold text-muted-foreground">请求</Text>
        <IconButton icon="plus" label="新建请求文件" onPress={() => naming.set({ kind: "create" })} />
      </View>
      <If when={naming()?.kind === "create"}>
        <NameInput initial="" placeholder="文件名，如 users/list" onDone={(name) => (name ? createFile(name) : naming.set(null))} />
      </If>
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
              const editing = () => {
                const n = naming();
                return (n?.kind === "request" || n?.kind === "title") && n.file === r.request.file && n.index === r.request.index ? n : null;
              };
              return (
                <If
                  when={!editing()}
                  else={
                    <If
                      when={editing()?.kind === "title"}
                      else={
                        <NameInput
                          initial={r.request.name ?? ""}
                          placeholder="请求名（# @name），引用时用：不能有空格"
                          onDone={(name) => {
                            if (name && name !== (r.request.name ?? "")) renameRequest(r.request.file, r.request.index, name);
                            else naming.set(null);
                            afterNaming(r.request.index);
                          }}
                        />
                      }
                    >
                      <NameInput
                        initial={r.request.title ?? ""}
                        placeholder="标题，写什么都行"
                        onDone={(title) => {
                          const fresh = (editing() as { fresh?: boolean } | null)?.fresh ?? false;
                          if (title !== null && title !== (r.request.title ?? "")) setTitle(r.request.file, r.request.index, title, fresh);
                          else naming.set(null);
                          afterNaming(r.request.index);
                        }}
                      />
                    </If>
                  }
                >
                <View
                  onContextMenu={(e) => requestMenu(r.request, e.pagePosition)}
                  role="button"
                  accessibilityLabel={`${r.request.method} ${requestLabel(r.request)}`}
                  onPress={() => selectRequest(r.request)}
                  class={`h-7 flex-row items-center gap-1.5 rounded-md pr-2 ${active() ? "border border-list-active-border bg-list-active" : "hover:bg-list-hover"}`}
                  style={{ paddingLeft: pad }}
                >
                  <Method method={r.request.method} />
                  <Text class="shrink text-[13px] text-foreground" style={{ numberOfLines: 1 }}>
                    {requestLabel(r.request)}
                  </Text>
                  <If when={r.request.title && r.request.name}>
                    <Text class="flex-1 font-mono text-[11px] text-muted-foreground" style={{ numberOfLines: 1 }}>{r.request.name!}</Text>
                  </If>
                  <If when={!(r.request.title && r.request.name)}>
                    <View class="flex-1" />
                  </If>
                </View>
                </If>
              );
            }
            const renaming = () => {
              const n = naming();
              return r.kind === "file" && n?.kind === "rename" && n.file === r.file;
            };
            const file = r.kind === "file" ? r.file : "";
            // Read when it is used: the row stays while its folding changes.
            const isOpen = () => !closed().has(r.key);
            return (
              <If
                when={!renaming()}
                else={<NameInput initial={file} placeholder="新的文件名" onDone={(name) => (name && name !== file ? renameFile(file, name) : naming.set(null))} />}
              >
              <View
                role="button"
                accessibilityLabel={r.label}
                onContextMenu={(e) => r.kind === "file" && fileMenu(r.file, e.pagePosition)}
                onPress={() => {
                  if (r.kind === "folder") return toggle(r.key);
                  // A file: open it (and show its requests); pressed again while it is the one open, fold it.
                  if (activeFile() === r.file && isOpen()) return toggle(r.key);
                  openFile(r.file);
                  if (!isOpen()) toggle(r.key);
                }}
                class="h-7 flex-row items-center gap-1.5 rounded-md pr-2 hover:bg-list-hover"
                style={{ paddingLeft: r.kind === "folder" ? pad : pad - 14 }}
              >
                <View
                  role="button"
                  accessibilityLabel={isOpen() ? `收起 ${r.label}` : `展开 ${r.label}`}
                  onPress={() => toggle(r.key)}
                  class="h-5 w-4 items-center justify-center rounded hover:bg-list-active"
                  style={{ transform: [{ rotate: isOpen() ? "90deg" : "0deg" }] }}
                >
                  <Icon name="chevronRight" size={12} />
                </View>
                <Icon name={r.kind === "folder" ? "folder" : "file"} />
                <Text class="flex-1 text-[13px] text-foreground" style={{ numberOfLines: 1 }}>{r.label}</Text>
              </View>
              </If>
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

function requestMenu(r: RequestRef, at: { x: number; y: number }) {
  Menu.popup(
    [
      // Not selecting it: that moves the editor's caret, which takes the focus from the name field.
      { label: "重命名…", onSelect: () => naming.set({ kind: "title", file: r.file, index: r.index }) },
      { label: "修改请求名（@name）…", onSelect: () => naming.set({ kind: "request", file: r.file, index: r.index }) },
      { label: "复制为 curl", onSelect: () => (selectRequest(r), copyCurl()) },
    ],
    at,
  );
}

function fileMenu(file: string, at: { x: number; y: number }) {
  Menu.popup(
    [
      { label: "重命名…", onSelect: () => naming.set({ kind: "rename", file }) },
      { label: "在 Finder 中显示", onSelect: () => void api.reveal(file).catch(() => {}) },
      { type: "separator" },
      { label: "移到废纸篓", onSelect: () => void trashFile(file) },
    ],
    at,
  );
}

/** A name typed in place: Return keeps it, Escape or leaving the field drops it. */
function NameInput(props: { initial: string; placeholder: string; onDone: (name: string | null) => void }) {
  const value = signal(props.initial);
  let done = false;
  const finish = (name: string | null) => {
    if (done) return;
    done = true;
    props.onDone(name);
  };
  return (
    <View class="mx-1.5 h-7 justify-center rounded-md border border-brand bg-background px-2">
      <Input
        value={value()}
        onChange={({ value: v }) => value.set(v)}
        onSubmit={() => finish(value().trim() || null)}
        submitBehavior="submit"
        // Return blurs before it submits on some platforms: let the submit come first.
        onBlur={() => setTimeout(() => finish(null), 100)}
        onKeyPress={({ key }) => (key === "Enter" ? finish(value().trim() || null) : key === "Escape" ? finish(null) : undefined)}
        autoFocus
        testID="name-input"
        placeholder={props.placeholder}
        class="font-mono text-xs text-foreground placeholder:text-muted-foreground"
      />
    </View>
  );
}
