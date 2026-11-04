// The environment view: the two environment files edited as text, and the variables an environment ends up with,
// with where each comes from (secrets masked). Read-only on the right: the files are the source.
import { computed, signal, Text, View, Scroll } from "nsh";
import { Editor } from "nsh/base";
import { api } from "./api";
import { Icon } from "./icons";
import { env, envState, envTab, reloadEnv } from "./store";
import { Tabs } from "./ui";

const FILES = { public: "http-client.env.json", private: "http-client.private.env.json" } as const;
const TEMPLATE = '{\n  "$shared": {},\n  "dev": {\n    "baseUrl": "http://localhost:8080"\n  }\n}\n';

const texts = signal<Record<string, string>>({});
const preview = signal<string | null>(null);

function read(file: string) {
  try {
    return api.read(file);
  } catch {
    return "";
  }
}

let timer: ReturnType<typeof setTimeout> | undefined;

function edit(file: string, value: string) {
  texts.set({ ...texts(), [file]: value });
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => {
    api.write(file, value);
    reloadEnv();
  }, 500);
}

const sourceLabel: Record<string, string> = { shared: "$shared", env: "环境", private: "私有", override: "覆盖" };

export function EnvPane() {
  const file = () => FILES[envTab()];
  const value = () => texts()[file()] ?? read(file());
  const shownEnv = () => preview() ?? env() ?? envState().envs[0] ?? null;
  const view = computed(() => {
    void envState();
    try {
      return api.envView(shownEnv());
    } catch (e) {
      return { envs: [], vars: [], error: String(e instanceof Error ? e.message : e) } as ReturnType<typeof api.envView> & { error?: string };
    }
  });
  return (
    <View class="min-w-0 flex-1 flex-row">
      <View class="min-w-0 flex-1 bg-background">
        <View class="h-9 flex-row border-b border-border bg-tab-bar">
          <Each of={["public", "private"] as const}>
            {(t) => (
              <View
                role="tab"
                accessibilityState={{ selected: envTab() === t() }}
                onPress={() => envTab.set(t())}
                class={`flex-row items-center gap-2 border-r border-border px-3.5 ${envTab() === t() ? "border-t-2 border-t-brand bg-tab-active" : ""}`}
              >
                <If when={t() === "private"}>
                  <Icon name="lock" size={12} />
                </If>
                <Text class={`text-[13px] ${envTab() === t() ? "text-foreground" : "text-muted-foreground"}`}>{FILES[t()]}</Text>
                <Text class="text-xs text-muted-foreground">{t() === "public" ? "共享 · 提交到 git" : "私有 · 不要提交"}</Text>
              </View>
            )}
          </Each>
        </View>
        <View class="h-11 flex-row items-center gap-2.5 border-b border-border px-3">
          <Text class="text-[13px] font-semibold text-foreground">环境变量</Text>
          <Text class="flex-1 text-xs text-muted-foreground" style={{ numberOfLines: 1 }}>
            每个顶层键是一个环境，$shared 里的变量所有环境都能用；同名时 私有 &gt; 环境 &gt; $shared
          </Text>
        </View>
        <Editor
          value={value() || (envTab() === "public" ? TEMPLATE : '{\n  "dev": {}\n}\n')}
          onChange={({ value }) => edit(file(), value)}
          language="json"
          class="flex-1"
        />
      </View>
      <View class="w-[500px] border-l border-border bg-panel">
        <View class="h-11 flex-row items-center gap-2.5 border-b border-border px-3">
          <Text class="flex-1 text-[13px] font-semibold text-foreground">生效变量</Text>
          <View class="flex-row gap-0.5 rounded-md bg-tab-bar p-0.5">
            <Each of={envState().envs}>
              {(e) => (
                <View
                  role="radio"
                  accessibilityState={{ checked: shownEnv() === e() }}
                  onPress={() => preview.set(e())}
                  class={`h-6 justify-center rounded px-2.5 ${shownEnv() === e() ? "bg-tab-active" : ""}`}
                >
                  <Text class={`text-xs ${shownEnv() === e() ? "font-semibold text-foreground" : "text-muted-foreground"}`}>{e()}</Text>
                </View>
              )}
            </Each>
          </View>
        </View>
        <View class="flex-row gap-3 border-b border-border px-3 py-2">
          <Text class="w-[130px] text-[11px] text-muted-foreground">变量</Text>
          <Text class="flex-1 text-[11px] text-muted-foreground">值</Text>
          <Text class="w-14 text-right text-[11px] text-muted-foreground">来源</Text>
        </View>
        <If when={(view() as { error?: string }).error}>
          <Text class="px-3 py-2 text-xs text-status-err">{(view() as { error?: string }).error!}</Text>
        </If>
        <Scroll class="flex-1">
          <Each of={view().vars}>
            {(v) => (
              <View class="flex-row items-center gap-3 border-b border-row-border px-3 py-2">
                <Text class="w-[130px] font-mono text-xs text-variable" style={{ numberOfLines: 1 }}>{v().name}</Text>
                <Text class="flex-1 font-mono text-xs text-foreground" style={{ numberOfLines: 1 }}>{v().value}</Text>
                <Text class="w-14 text-right text-xs text-muted-foreground">{sourceLabel[v().source] ?? v().source}</Text>
              </View>
            )}
          </Each>
        </Scroll>
        <View class="gap-1 border-t border-border px-3 py-2.5">
          <Text class="text-[11px] text-muted-foreground">私有文件里的值是密钥：这里显示为 ••••••，CLI 和 MCP 输出时显示为 ***。</Text>
          <Text class="text-[11px] text-muted-foreground">请确认 .gitignore 里有 http-client.private.env.json。</Text>
        </View>
      </View>
    </View>
  );
}
