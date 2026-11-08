/** The dog head and the "mddog" wordmark. The home navbar and the docs sidebar both render this. */
export function Logo() {
  return (
    <span className="inline-flex items-center gap-2">
      <img src="/head.png" alt="" width={28} height={28} className="size-7 shrink-0" />
      <span className="font-mono text-[15px] font-bold leading-none tracking-tight">
        md<span className="text-(--mddog-grey)">dog</span>
      </span>
    </span>
  );
}
