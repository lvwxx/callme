// The window's title bar, drawn by the app: the project, the command palette's entry, the environment.
import { Text, View } from "nsh";
import { isAvailable as hasWindow, Menu, titleBar } from "nsh/desktop";
import { api, project } from "./api";
import { Icon } from "./icons";
import { creatingEnv, env, envState, envTab, openProject, paletteOpen, pickProject, saveNow, setEnv, view } from "./store";
import { Kbd, Tip } from "./ui";

const name = () => project().split("/").filter(Boolean).pop() ?? project();
const home = () => project().replace(/^\/Users\/[^/]+/, "~");

/** The project's menu: the recent projects, opening a folder, showing this one in the Finder. */
function projectMenu() {
  const short = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");
  const recent = api.recent();
  Menu.popup([
    ...(recent.length
      ? recent.map((p) => ({ label: short(p), checked: p === project(), onSelect: () => p !== project() && openProject(p) }))
      : [{ label: "还没有最近的项目", disabled: true }]),
    { type: "separator" as const },
    { label: "打开文件夹…", shortcut: "mod+o", onSelect: pickProject },
    { label: "在 Finder 中显示", disabled: !project(), onSelect: () => void api.reveal("").catch(() => {}) },
  ]);
}

export function TitleBar() {
  const pickEnv = () => {
    const envs = envState().envs;
    const newEnv = () => {
      saveNow();
      envTab.set("public");
      view.set("env");
      creatingEnv.set(true);
    };
    Menu.popup([
      ...(envs.length
        ? envs.map((e) => ({ label: e, checked: e === env(), onSelect: () => setEnv(e) }))
        : [{ label: "还没有环境", disabled: true }]),
      { type: "separator" as const },
      { label: "新建环境…", onSelect: newEnv },
      { label: "编辑环境变量", onSelect: () => (saveNow(), view.set("env")) },
    ]);
  };
  return (
    <View
      desktop={{ windowDrag: true }}
      class="flex-row items-center gap-3 border-b border-title-bar-border bg-title-bar pr-3"
      style={{ height: (hasWindow && titleBar().height) || 44, paddingLeft: (hasWindow ? titleBar().left : 0) + 12 }}
    >
      <Icon name="logo" size={24} color="brand" weight={2.68} />
      <View
        role="button"
        accessibilityLabel="项目"
        onPress={projectMenu}
        tooltip={<Tip label={project() ? home() : "还没有打开项目"} />}
        class="h-7 flex-row items-center gap-1 rounded-md px-1.5 hover:bg-list-hover"
      >
        <Text class="text-[13px] font-semibold text-foreground">{project() ? name() : "callme"}</Text>
        <Icon name="chevronDown" size={12} />
      </View>
      <View class="flex-1 items-center">
        <View
          role="button"
          accessibilityLabel="命令面板"
          onPress={() => paletteOpen.set(true)}
          class="h-7 w-[420px] max-w-full flex-row items-center gap-2 rounded-md border border-border bg-background px-2.5"
        >
          <Icon name="search" />
          <Text class="flex-1 text-[13px] text-muted-foreground">搜索请求、命令，或粘贴 curl…</Text>
          <Kbd>⌘K</Kbd>
        </View>
      </View>
      <View
        role="button"
        accessibilityLabel={`切换环境，当前 ${env() ?? "无"}`}
        onPress={pickEnv}
        class="h-7 flex-row items-center gap-1.5 rounded-md border border-border bg-background px-2.5"
      >
        <View class={`h-2 w-2 rounded-full ${env() ? "bg-brand" : "bg-muted-foreground"}`} />
        <Text class="text-[13px] font-semibold text-foreground">{env() ?? "未选择环境"}</Text>
        <Icon name="chevronDown" size={12} />
      </View>
    </View>
  );
}
