import { useEffect, useState } from "preact/hooks";
import type { MediaApi, MediaItem } from "./api.ts";

export interface ThumbProps {
  api: MediaApi;
  item: MediaItem;
}

/** サムネイル。画像はアプリから JPEG のバイト列で受け、Blob の URL にして出す。無いときは種類を字で出す。 */
export const Thumb = ({ api, item }: ThumbProps) => {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!item.has_thumbnail || typeof URL.createObjectURL !== "function") {
      setUrl(null);
      return undefined;
    }
    let made: string | null = null;
    let alive = true;
    api
      .thumbnail(item.id)
      .then((bytes) => {
        if (!alive) return;
        made = URL.createObjectURL(new Blob([bytes], { type: "image/jpeg" }));
        setUrl(made);
      })
      .catch(() => alive && setUrl(null));
    return () => {
      alive = false;
      if (made) URL.revokeObjectURL(made);
    };
  }, [api, item.id, item.has_thumbnail, item.content_hash]);

  return url ? (
    <img class="mm-media-thumb" src={url} alt="" />
  ) : (
    <span class="mm-media-thumb mm-media-thumb-empty" aria-hidden="true">
      {item.kind === "audio" ? "Audio" : "No preview"}
    </span>
  );
};
