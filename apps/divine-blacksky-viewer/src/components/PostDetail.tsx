import { useEffect, useRef } from "react";
import Hls from "hls.js";

import type { PostView } from "../api";
import { deriveDirectPlaybackUrl } from "../playback";

interface PostDetailProps {
  post: PostView;
  onOpenAuthor: (actor: string) => void;
}

export function PostDetail({ post, onOpenAuthor }: PostDetailProps) {
  const videoRef = useRef<HTMLVideoElement | null>(null);

  useEffect(() => {
    const video = videoRef.current;
    const playlist = post.embed?.playlist;
    if (!video || !playlist) return;

    const directPlaybackUrl = deriveDirectPlaybackUrl(playlist);

    if (directPlaybackUrl) {
      video.src = directPlaybackUrl;
      return () => {
        video.pause();
        video.removeAttribute("src");
        video.load();
      };
    }

    if (playlist.endsWith(".m3u8") && Hls.isSupported()) {
      const hls = new Hls({ enableWorker: true });
      hls.loadSource(playlist);
      hls.attachMedia(video);
      return () => hls.destroy();
    }

    video.src = playlist;
    return () => {
      video.pause();
      video.removeAttribute("src");
      video.load();
    };
  }, [post.embed?.playlist]);

  return (
    <section className="detail-panel">
      {post.embed ? (
        <div className="detail-video-wrap">
          <div className="video-shell">
            <video
              autoPlay
              controls
              muted
              playsInline
              poster={post.embed.thumbnail ?? undefined}
              ref={videoRef}
            />
          </div>
        </div>
      ) : null}

      <div className="detail-info">
        <div className="detail-author-row">
          {post.author.avatar ? (
            <img alt="" className="card-avatar" src={post.author.avatar} />
          ) : (
            <div className="card-avatar-placeholder">
              {(post.author.displayName ?? post.author.handle).charAt(0).toUpperCase()}
            </div>
          )}
          <div>
            <button
              className="detail-author-btn"
              onClick={() => onOpenAuthor(post.author.did)}
              type="button"
            >
              {post.author.displayName ?? post.author.handle}
            </button>
            <p className="detail-handle">@{post.author.handle}</p>
          </div>
        </div>

        <p className="detail-text">{post.text || "No text content."}</p>
        <p className="detail-timestamp">
          {new Date(post.createdAt).toLocaleString()}
        </p>
      </div>
    </section>
  );
}
