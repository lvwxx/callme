// The window's title bar, drawn by the app: the project, the command palette's entry, the environment.
import { Text, View } from "nsh";
import { isAvailable as hasWindow, Menu, titleBar } from "nsh/desktop";
import { project } from "./api";
import { Icon } from "./icons";
import { creatingEnv, env, envState, envTab, paletteOpen, saveNow, setEnv, view } from "./store";
import { Kbd } from "./ui";

const name = () => project().split("/").filter(Boolean).pop() ?? project();
const home = () => project().replace(/^\/Users\/[^/]+/, "~");

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
      <View class="h-5 w-5 items-center justify-center rounded bg-brand">
        <Text class="text-xs font-extrabold text-[#0a0e19]">c</Text>
      </View>
      <Text class="text-[13px] font-semibold text-foreground">{project() ? name() : "callme"}</Text>
      <Text class="text-xs text-muted-foreground" style={{ numberOfLines: 1 }}>{home()}</Text>
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
