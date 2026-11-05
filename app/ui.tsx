// Small pieces the panes share.
import { type JSX, Text, View } from "nsh";
import { Tooltip } from "nsh/base";
import { Icon, type IconName } from "./icons";

const methodClass: Record<string, string> = {
  GET: "text-method-get",
  POST: "text-method-post",
  PUT: "text-method-put",
  PATCH: "text-method-patch",
  DELETE: "text-method-delete",
};

export function Method(props: { method: string; class?: string }) {
  return (
    <Text class={`w-11 font-mono text-[10px] font-bold ${methodClass[props.method] ?? "text-muted-foreground"} ${props.class ?? ""}`}>
      {props.method}
    </Text>
  );
}

export function Status(props: { status: number }) {
  const ok = () => props.status < 400;
  return (
    <View class={`rounded-md px-2 py-0.5 ${ok() ? "bg-status-ok-bg" : "bg-status-err-bg"}`}>
      <Text class={`font-mono text-xs font-bold ${ok() ? "text-status-ok" : "text-status-err"}`}>{String(props.status)}</Text>
    </View>
  );
}

export function SectionLabel(props: { children: JSX.Element }) {
  return <Text class="px-2 pb-1.5 text-[11px] font-semibold text-muted-foreground">{props.children}</Text>;
}

/** The hint an icon button shows when the pointer rests on it: its name, and its shortcut. */
export function Tip(props: { label: string; shortcut?: string; placement?: "top" | "bottom" | "left" | "right"; delay?: number }) {
  return (
    <Tooltip placement={props.placement} delay={props.delay} class="flex-row items-center gap-2 rounded-md bg-foreground px-2 py-1">
      <Text class="text-xs text-background">{props.label}</Text>
      <If when={props.shortcut}>
        <Text class="font-mono text-[11px] text-background opacity-60">{props.shortcut!}</Text>
      </If>
    </Tooltip>
  );
}

/** A borderless icon button: its name and shortcut show as a tooltip. */
export function IconButton(props: { icon: IconName; label: string; shortcut?: string; onPress: () => void; size?: number }) {
  return (
    <View
      role="button"
      accessibilityLabel={props.label}
      onPress={props.onPress}
      tooltip={<Tip label={props.label} shortcut={props.shortcut} />}
      class="h-7 w-7 items-center justify-center rounded-md hover:bg-list-hover active:bg-list-active"
    >
      <Icon name={props.icon} size={props.size ?? 15} />
    </View>
  );
}

/** The one filled button of the request bar: 发送, and 停止 while the request runs. */
export function SendButton(props: { running: boolean; disabled: boolean; onPress: () => void; onStop: () => void }) {
  return (
    <If
      when={props.running}
      else={
        <View
          role="button"
          accessibilityLabel="发送"
          accessibilityState={{ disabled: props.disabled }}
          onPress={() => !props.disabled && props.onPress()}
          tooltip={<Tip label="发送当前请求" shortcut="⌘↵" />}
          class={`h-7 flex-row items-center gap-2 rounded-md pl-3 pr-2.5 ${props.disabled ? "bg-muted" : "bg-brand hover:opacity-90 active:opacity-80"}`}
        >
          <Icon name="play" size={12} color={props.disabled ? "muted-foreground" : "#0a0e19"} />
          <Text class={`text-[13px] font-semibold ${props.disabled ? "text-muted-foreground" : "text-[#0a0e19]"}`}>发送</Text>
          <View class={`rounded px-1 ${props.disabled ? "" : "bg-[#0a0e1914]"}`}>
            <Text class={`font-mono text-[11px] ${props.disabled ? "text-muted-foreground" : "text-[#0a0e19]"}`}>⌘↵</Text>
          </View>
        </View>
      }
    >
      <View
        role="button"
        accessibilityLabel="停止"
        onPress={props.onStop}
        tooltip={<Tip label="停止这个请求，保留已收到的内容" shortcut="⌘." />}
        class="h-7 flex-row items-center gap-2 rounded-md border border-border bg-background pl-3 pr-2.5 hover:bg-list-hover active:bg-list-active"
      >
        <Icon name="stop" size={12} color="foreground" />
        <Text class="text-[13px] font-semibold text-foreground">停止</Text>
        <View class="rounded bg-muted px-1">
          <Text class="font-mono text-[11px] text-muted-foreground">⌘.</Text>
        </View>
      </View>
    </If>
  );
}

/** A thin vertical rule between groups of a toolbar. */
export function Divider() {
  return <View class="mx-1.5 h-[18px] w-px bg-border" />;
}

export function Button(props: { label: string; onPress: () => void; primary?: boolean; disabled?: boolean; hint?: string }) {
  return (
    <View
      role="button"
      accessibilityLabel={props.label}
      onPress={() => !props.disabled && props.onPress()}
      class={`h-7 flex-row items-center gap-2 rounded-md px-3 ${
        props.primary ? (props.disabled ? "bg-muted" : "bg-button active:opacity-80") : "border border-border hover:bg-list-hover"
      }`}
    >
      <Text class={`text-[13px] ${props.primary ? (props.disabled ? "text-muted-foreground" : "font-semibold text-button-foreground") : "text-foreground"}`}>
        {props.label}
      </Text>
      <If when={props.hint}>
        <Text class={`font-mono text-[11px] ${props.primary ? "text-button-foreground opacity-60" : "text-muted-foreground"}`}>{props.hint}</Text>
      </If>
    </View>
  );
}

export function Kbd(props: { children: string }) {
  return (
    <View class="rounded border border-border bg-muted px-1.5">
      <Text class="font-mono text-[11px] text-muted-foreground">{props.children}</Text>
    </View>
  );
}

/** A row of tabs drawn as text with a teal underline under the selected one. */
export function Tabs<T extends string>(props: { tabs: { id: T; label: string }[]; value: T; onChange: (id: T) => void }) {
  return (
    <View class="h-9 flex-row gap-1 border-b border-border px-2" role="tablist">
      <Each of={props.tabs}>
        {(t) => (
          <View
            role="tab"
            accessibilityState={{ selected: props.value === t().id }}
            onPress={() => props.onChange(t().id)}
            class={`justify-center border-b-2 px-2.5 ${props.value === t().id ? "border-brand" : "border-transparent"}`}
          >
            <Text class={`text-xs ${props.value === t().id ? "font-semibold text-foreground" : "text-muted-foreground"}`}>{t().label}</Text>
          </View>
        )}
      </Each>
    </View>
  );
}

export function size(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(2)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}
