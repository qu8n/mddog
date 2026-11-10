import { Pause, Play } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

/** Hero video. Plays muted on a loop unless the visitor asks for reduced motion, and can be paused. */
export function DemoVideo() {
  const ref = useRef<HTMLVideoElement>(null);
  const [playing, setPlaying] = useState(false);

  useEffect(() => {
    const video = ref.current;
    if (!video || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    video.play().catch(() => {});
  }, []);

  function toggle() {
    const video = ref.current;
    if (!video) return;
    if (video.paused) video.play().catch(() => {});
    else video.pause();
  }

  return (
    <div className="relative">
      <video
        ref={ref}
        src="/mddog-demo.mp4"
        muted
        loop
        playsInline
        preload="metadata"
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        aria-label="The same Markdown file shown raw with cat, then rendered by mddog with tables, diagrams, math, and images"
        className="block aspect-video w-full"
      />
      <button
        type="button"
        onClick={toggle}
        aria-label={playing ? 'Pause video' : 'Play video'}
        className="absolute right-3 bottom-3 rounded-full bg-black/60 p-2 text-white/90 backdrop-blur-sm transition-colors hover:bg-black/80 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white"
      >
        {playing ? <Pause className="size-4" /> : <Play className="size-4" />}
      </button>
    </div>
  );
}
