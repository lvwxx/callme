// The command palette (⌘K): requests and commands by what is typed (`>` commands only, `@` requests only), and a
// pasted curl command or fetch(…) call turned into a request, with the environment's variables in place of its values.
import { computed, keymap, Overlay, Scroll, signal, Text, token, View } from "nsh";
import { Input } from "nsh/base";
import { api } from "./api";
import { Icon } from "./icons";
import {
  activeFile,
  copyCurl,
  env,
  envState,
  envTab,
  files,
  openFile,
  paletteOpen,
  refresh,
  requests,
  saveNow,
  selectRequest,
  send,
  setEnv,
  setText,
  text,
  view,
} from "./store";
import { Kbd, Method } from "./ui";

type Item = { key: string; section: string; label: string; detail?: string; method?: string; shortcut?: string; run: () => void };

const query = signal("");
const selected = signal(0);

function close() {
  paletteOpen.set(false);
  query.set("");
  selected.set(0);
}

/** A file name for an imported request: the URL's last path segment. */
function fileFor(url: string): string {
  const path = url.replace(/^\{\{[^}]+\}\}/, "").replace(/^[a-z]+:\/\/[^/]+/i, "").split(/[?#]/)[0];
  const segment = path.split("/").filter((s) => s && !s.startsWith("{{")).pop() ?? "request";
  const base = segment.replace(/[^\w.-]/g, "") || "request";
  let name = `${base}.http`;
  for (let i = 2; files().includes(name); i++) name = `${base}-${i}.http`;
  return name;
}

const imported = computed(() => {
  const q = query().trim();
  if (!q.startsWith("curl") && !q.includes("fetch(")) return null;
  try {
    return api.importText(q, env());
  } catch (e) {
    return { error: String(e instanceof Error ? e.message : e) };
  }
});

const items = computed<Item[]>(() => {
  const imp = imported();
  if (imp) {
    if ("error" in imp) return [];
    const urlLine = imp.text.split("\n").find((l) => /^[A-Z]+ /.test(l)) ?? "";
    const file = fileFor(urlLine.split(" ")[1] ?? "");
    const out: Item[] = [
      {
        key: "new",
        section: "操作",
        label: `新建 ${file}`,
        shortcut: "↵",
        run: () => {
          api.write(file, imp.text);
          refresh();
          openFile(file);
          close();
        },
      },
    ];
    if (activeFile()) {
      out.push({
        key: "append",
        section: "操作",
        label: `追加到当前文件 ${activeFile()}`,
        run: () => {
          const t = text().replace(/\s*$/, "");
          setText(`${t}${t ? "\n\n###\n" : ""}${imp.text}`);
          saveNow();
          close();
        },
      });
    }
    return out;
  }
  const q = query().trim();
  const onlyCommands = q.startsWith(">");
  const onlyRequests = q.startsWith("@");
  const needle = (onlyCommands || onlyRequests ? q.slice(1) : q).trim().toLowerCase();
  const match = (...xs: (string | null | undefined)[]) => !needle || xs.some((x) => x?.toLowerCase().includes(needle));
  const out: Item[] = [];
  if (!onlyCommands) {
    for (const r of requests()) {
      if (match(r.name, r.title, r.url, r.file))
        out.push({
          key: `r:${r.file}#${r.id}`,
          section: "请求",
          label: r.name ?? r.title ?? r.id,
          detail: `${r.file} · ${r.url}`,
          method: r.method,
          run: () => (selectRequest(r), close()),
        });
    }
  }
  if (!onlyRequests) {
    const commands: Item[] = [
      { key: "send", section: "命令", label: "发送当前请求", shortcut: "⌘↵", run: () => (close(), send()) },
      { key: "curl", section: "命令", label: "复制当前请求为 curl", shortcut: "⌘⇧C", run: () => (copyCurl(), close()) },
      { key: "env", section: "命令", label: "打开环境变量", run: () => (envTab.set("public"), view.set("env"), close()) },
      ...envState()
        .envs.filter((e) => e !== env())
        .map((e) => ({ key: `env:${e}`, section: "命令", label: `切换环境 → ${e}`, run: () => (setEnv(e), close()) })),
    ];
    out.push(...commands.filter((c) => match(c.label)));
  }
  return out.slice(0, 50);
});

function runSelected() {
  const list = items();
  list[Math.min(selected(), list.length - 1)]?.run();
}

export function Palette() {
  keymap.bind(
    {
      up: () => paletteOpen() && selected.set(Math.max(0, selected() - 1)),
      down: () => paletteOpen() && selected.set(Math.min(items().length - 1, selected() + 1)),
    },
    { when: "always" },
  );
  return (
    <If when={paletteOpen()}>
      <View class="absolute inset-0 bg-black/40" onPress={close} />
      <Overlay onCancel={close} onPressOutside={close} class="overflow-hidden rounded-lg border border-border bg-popover" style={{ position: "absolute", top: 96, left: "50%", marginLeft: -340, width: 680 }}>
        <View class="flex-row items-center gap-2.5 border-b border-border px-4 py-3">
          <Icon name="search" size={16} />
          <Input
            value={query()}
            onChange={({ value }) => (query.set(value), selected.set(0))}
            onSubmit={runSelected}
            submitBehavior="submit"
            autoFocus
            testID="palette-input"
            multiline={!!imported()}
            placeholder="搜索请求和命令，> 只搜命令，@ 只搜请求；也可以粘贴 curl"
            class="flex-1 font-mono text-[13px] text-foreground"
            style={{ placeholderColor: token("muted-foreground" as never) as string }}
          />
        </View>
        <If when={imported()}>
          {(imp) => (
            <View class="border-b border-border px-4 py-3">
              <If when={"error" in imp()} else={<ImportPreview text={(imp() as { text: string }).text} replaced={(imp() as { replaced: string[] }).replaced} />}>
                <Text class="text-xs text-status-err">{(imp() as { error: string }).error}</Text>
              </If>
            </View>
          )}
        </If>
        <Scroll class="max-h-[360px] p-1.5">
          <Each of={items()}>
            {(it, i) => (
              <>
                <If when={i() === 0 || items()[i() - 1]?.section !== it().section}>
                  <Text class="px-2.5 pb-1 pt-2 text-[11px] font-semibold text-muted-foreground">{it().section}</Text>
                </If>
                <View
                  role="option"
                  accessibilityState={{ selected: i() === selected() }}
                  onPress={() => it().run()}
                  class={`h-[34px] flex-row items-center gap-2.5 rounded-md px-2.5 ${i() === selected() ? "border border-list-active-border bg-list-active" : ""}`}
                >
                  <If when={it().method}>
                    <Method method={it().method!} />
                  </If>
                  <Text class="text-[13px] text-foreground" style={{ numberOfLines: 1 }}>{it().label}</Text>
                  <Text class="flex-1 font-mono text-xs text-muted-foreground" style={{ numberOfLines: 1 }}>{it().detail ?? ""}</Text>
                  <If when={it().shortcut}>
                    <Kbd>{it().shortcut!}</Kbd>
                  </If>
                </View>
              </>
            )}
          </Each>
          <If when={items().length === 0 && !imported()}>
            <Text class="px-3 py-3 text-xs text-muted-foreground">没有匹配的请求或命令</Text>
          </If>
        </Scroll>
        <View class="flex-row gap-4 border-t border-border px-4 py-2">
          <Text class="text-[11px] text-muted-foreground">↑↓ 选择</Text>
          <Text class="text-[11px] text-muted-foreground">↵ 执行</Text>
          <Text class="text-[11px] text-muted-foreground">esc 关闭</Text>
        </View>
      </Overlay>
    </If>
  );
}

function ImportPreview(props: { text: string; replaced: string[] }) {
  return (
    <View class="gap-2">
      <View class="flex-row justify-between">
        <Text class="text-[11px] font-semibold text-muted-foreground">识别到请求 · 转换结果</Text>
        <Text class="text-[11px] text-muted-foreground">{props.replaced.length ? `已替换为变量：${props.replaced.join("、")}` : ""}</Text>
      </View>
      <View class="rounded-md border border-border bg-background px-3 py-2.5">
        <Text class="font-mono text-xs text-foreground">{props.text.trimEnd()}</Text>
      </View>
    </View>
  );
}
