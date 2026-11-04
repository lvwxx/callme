// callme: a simple, AI-friendly API client for .http files. The title bar, the sidebar, the request and response
// panes (or the environment view), a status bar, and the command palette over them.
import { computed, effect, keymap, mount, Text, View } from "nsh";
import { isAvailable as hasWindow, Menu, Window } from "nsh/desktop";
import { root } from "./api";
import { EnvPane } from "./EnvPane";
import { Palette } from "./Palette";
import { RequestPane } from "./RequestPane";
import { clearPicked, ResponsePane } from "./ResponsePane";
import { Sidebar } from "./Sidebar";
import {
  activeFile,
  checkExternalChanges,
  copyCurl,
  currentIndex,
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
        { label: "发送", shortcut: "mod+enter", onSelect: () => send() },
        { label: "复制为 curl", shortcut: "mod+shift+c", onSelect: copyCurl },
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
  },
  { when: "always" },
);

// Files change under the app (an agent edits them): look once a second.
setInterval(checkExternalChanges, 1000);

const first = requests()[0];
if (first) selectRequest(first);

function StatusBar() {
  const left = computed(() =>
    view() === "env" ? "环境变量" : activeFile() ? `${activeFile()} · 第 ${currentIndex() || "-"} 个请求` : root,
  );
  return (
    <View class="h-[26px] flex-row items-center gap-4 border-t border-border bg-title-bar px-3">
      <Text class="text-[11px] text-muted-foreground">{left()}</Text>
      <Text class="flex-1 text-[11px] text-brand" style={{ numberOfLines: 1 }}>{notice() ?? ""}</Text>
      <Text class="text-[11px] text-muted-foreground">{`环境 ${env() ?? "未选择"} · ${envState().vars.length} 个变量`}</Text>
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
      <View class="min-h-0 flex-1 flex-row">
        <Sidebar />
        <If when={view() === "env"} else={<><RequestPane /><ResponsePane /></>}>
          <EnvPane />
        </If>
      </View>
      <StatusBar />
      <Palette />
    </View>
  );
}

mount(App);
