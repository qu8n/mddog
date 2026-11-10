/** The dog head and the "mddog" wordmark. The home navbar and the docs sidebar both render this. */
export function Logo() {
  return (
    <span className="inline-flex items-center gap-1">
      {/* The head's weight sits in its top half, so drop it 2px to line up with the text by eye. */}
      <img src="/head.webp" alt="" width={28} height={28} className="size-7 shrink-0 translate-y-[2px]" />
      <span className="font-sans text-[15px] font-medium">
        md<span className="text-(--mddog-grey)">dog</span>
      </span>
    </span>
  );
}
