// callme: a simple, AI-friendly API client for .http files. The title bar, the sidebar, the request and response
// panes (or the environment view), a status bar, and the command palette over them.
import { computed, effect, keymap, mount, signal, Text, View } from "nsh";
import { isAvailable as hasWindow, Menu, Window } from "nsh/desktop";
import { Callme } from "callme/native";
import { api, project } from "./api";
import { EnvPane } from "./EnvPane";
import { Palette } from "./Palette";
import { RequestPane } from "./RequestPane";
import { clearPicked, ResponsePane } from "./ResponsePane";
import { Sidebar } from "./Sidebar";
import {
  activeFile,
  checkExternalChanges,
  naming,
  newRequest,
  pickProject,
  copyCurl,
  currentIndex,
  formatBody,
  currentRequest,
  env,
  envState,
  notice,
  paletteOpen,
  requests,
  saveNow,
  selectRequest,
  send,
  view,
} from "./store";
import { TitleBar } from "./TitleBar";

if (hasWindow) {
  Window.setTitle("callme");
  Window.setTitleBar({ style: "hidden", height: 44 });
  Menu.setApplicationMenu([
    {
      label: "callme",
      submenu: [{ role: "about" }, { type: "separator" }, { role: "hide" }, { role: "hideOthers" }, { type: "separator" }, { role: "quit" }],
    },
    {
      label: "请求",
      submenu: [
        { label: "新建请求", shortcut: "mod+n", onSelect: newRequest },
        { label: "新建请求文件…", shortcut: "mod+shift+n", onSelect: () => naming.set({ kind: "create" }) },
        { label: "打开文件夹…", shortcut: "mod+o", onSelect: pickProject },
        { type: "separator" },
        { label: "发送", shortcut: "mod+enter", onSelect: () => send() },
        { label: "复制为 curl", shortcut: "mod+shift+c", onSelect: copyCurl },
        { label: "格式化 JSON 请求体", shortcut: "mod+shift+f", onSelect: formatBody },
        { label: "保存", shortcut: "mod+s", onSelect: saveNow },
        { type: "separator" },
        { label: "命令面板…", shortcut: "mod+k", onSelect: () => paletteOpen.set(true) },
        { label: "环境变量", shortcut: "mod+e", onSelect: () => view.set(view() === "env" ? "request" : "env") },
      ],
    },
    { label: "编辑", submenu: [{ role: "undo" }, { role: "redo" }, { type: "separator" }, { role: "cut" }, { role: "copy" }, { role: "paste" }, { role: "selectAll" }] },
    { label: "窗口", submenu: [{ role: "minimize" }, { role: "zoom" }, { type: "separator" }, { role: "toggleFullScreen" }] },
  ]);
}

keymap.bind(
  {
    "mod+enter": () => send(),
    "mod+k": () => paletteOpen.set(!paletteOpen()),
    "mod+n": () => newRequest(),
    "mod+shift+f": () => formatBody(),
    "mod+shift+c": () => copyCurl(),
    "mod+shift+n": () => naming.set({ kind: "create" }),
  },
  { when: "always" },
);

// Files change under the app (an agent edits them): look once a second.
setInterval(checkExternalChanges, 1000);

// The project opened at launch heads the recent ones too.
if (project()) Callme.remember(project());
const first = requests()[0];
if (first) selectRequest(first);

/** A note when the project's .gitignore would let secrets or local state into git. */
function GitignoreNote() {
  const missing = signal<string[]>([]);
  effect(() => {
    if (project()) missing.set(api.gitignoreMissing());
  });
  return (
    <If when={missing().length > 0}>
      <View class="flex-row items-center gap-3 border-t border-warning bg-warning/10 px-3 py-1.5">
        <Text class="flex-1 text-xs text-foreground">{`.gitignore 里没有 ${missing().join("、")}：密钥和本地记录可能会被提交到 git`}</Text>
        <View role="button" accessibilityLabel="加到 .gitignore" onPress={() => (api.addToGitignore(missing()), missing.set([]), notice.set("已加到 .gitignore"))} class="rounded-md bg-button px-2.5 py-1 active:opacity-80">
          <Text class="text-xs font-semibold text-button-foreground">加到 .gitignore</Text>
        </View>
        <View role="button" accessibilityLabel="忽略" onPress={() => missing.set([])} class="rounded-md px-2 py-1 hover:bg-list-hover">
          <Text class="text-xs text-muted-foreground">忽略</Text>
        </View>
      </View>
    </If>
  );
}

function StatusBar() {
  const left = computed(() =>
    view() === "env" ? "环境变量" : activeFile() ? `${activeFile()} · 第 ${currentIndex() || "-"} 个请求` : project(),
  );
  return (
    <View class="h-[26px] flex-row items-center gap-4 border-t border-border bg-title-bar px-3">
      <Text class="text-[11px] text-muted-foreground">{left()}</Text>
      <Text class="flex-1 text-[11px] text-brand" style={{ numberOfLines: 1 }}>{notice() ?? ""}</Text>
      <Text class="text-[11px] text-muted-foreground">{`环境 ${env() ?? "未选择"} · ${envState().vars.length} 个变量`}</Text>
    </View>
  );
}

function Welcome() {
  return (
    <View class="flex-1 items-center justify-center gap-4 bg-background">
      <View class="h-12 w-12 items-center justify-center rounded-xl bg-brand">
        <Text class="text-2xl font-extrabold text-[#0a0e19]">c</Text>
      </View>
      <Text class="text-lg font-semibold text-foreground">callme</Text>
      <Text class="text-[13px] text-muted-foreground">打开一个放 .http 文件的文件夹，它就是一个项目</Text>
      <View role="button" accessibilityLabel="打开文件夹" onPress={pickProject} class="h-8 justify-center rounded-md bg-button px-4 active:opacity-80">
        <Text class="text-[13px] font-semibold text-button-foreground">打开文件夹…</Text>
      </View>
      <Text class="font-mono text-xs text-muted-foreground">或者在终端里：callme open .</Text>
    </View>
  );
}

function App() {
  // A picked history record belongs to the request it was picked for.
  effect(() => {
    void currentRequest();
    clearPicked();
  });
  return (
    <View class="flex-1 bg-background">
      <TitleBar />
      <If when={project()} else={<Welcome />}>
        <View class="min-h-0 flex-1 flex-row">
          <Sidebar />
          <If when={view() === "env"} else={<><RequestPane /><ResponsePane /></>}>
            <EnvPane />
          </If>
        </View>
      </If>
      <GitignoreNote />
      <StatusBar />
      <Palette />
    </View>
  );
}

mount(App);
