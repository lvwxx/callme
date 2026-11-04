// Icons drawn from Lucide's paths (https://lucide.dev, ISC License, Copyright (c) Lucide Contributors), stroked on a
// canvas as the nsh gallery does.
import { Canvas, Path2D, token } from "nsh";

type El =
  | { tag: "path"; d: string }
  | { tag: "circle"; cx: number; cy: number; r: number }
  | { tag: "rect"; x: number; y: number; width: number; height: number; rx: number };

const icons = {
  search: [{ tag: "path", d: "m21 21-4.34-4.34" }, { tag: "circle", cx: 11, cy: 11, r: 8 }],
  plus: [{ tag: "path", d: "M5 12h14" }, { tag: "path", d: "M12 5v14" }],
  folder: [{ tag: "path", d: "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" }],
  file: [
    { tag: "path", d: "M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" },
    { tag: "path", d: "M14 2v5a1 1 0 0 0 1 1h5" },
  ],
  chevronRight: [{ tag: "path", d: "m9 18 6-6-6-6" }],
  chevronDown: [{ tag: "path", d: "m6 9 6 6 6-6" }],
  lock: [{ tag: "rect", x: 3, y: 11, width: 18, height: 11, rx: 2 }, { tag: "path", d: "M7 11V7a5 5 0 0 1 10 0v4" }],
  copy: [{ tag: "rect", x: 8, y: 8, width: 14, height: 14, rx: 2 }, { tag: "path", d: "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" }],
  send: [{ tag: "path", d: "M5 12h14" }, { tag: "path", d: "m12 5 7 7-7 7" }],
  list: [{ tag: "path", d: "M2 5h20" }, { tag: "path", d: "M6 12h12" }, { tag: "path", d: "M9 19h6" }],
  x: [{ tag: "path", d: "M18 6 6 18" }, { tag: "path", d: "m6 6 12 12" }],
} satisfies Record<string, El[]>;

export type IconName = keyof typeof icons;

/** An icon on Lucide's 24 × 24 grid, stroked with `color` (a theme token name, default `muted-foreground`). */
export function Icon(props: { name: IconName; size?: number; color?: string }) {
  const size = props.size ?? 14;
  return (
    <Canvas
      style={{ pointerEvents: "none", width: size, height: size }}
      draw={(cx, { width }) => {
        cx.scale(width / 24, width / 24);
        cx.strokeStyle = token((props.color ?? "muted-foreground") as never) as string;
        cx.lineWidth = 2;
        cx.lineCap = "round";
        cx.lineJoin = "round";
        for (const el of icons[props.name] as El[]) {
          if (el.tag === "path") cx.stroke(new Path2D(el.d));
          else {
            cx.beginPath();
            if (el.tag === "circle") cx.arc(el.cx, el.cy, el.r, 0, Math.PI * 2);
            else cx.roundRect(el.x, el.y, el.width, el.height, el.rx);
            cx.stroke();
          }
        }
      }}
    />
  );
}
