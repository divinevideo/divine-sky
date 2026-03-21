import { useCallback, useEffect, useRef, useState } from "react";
import Hls from "hls.js";

import type { PostView } from "../api";
import { deriveDirectPlaybackUrl } from "../playback";

interface VideoCardProps {
  post: PostView;
  onOpenAuthor: (actor: string) => void;
  onOpenPost: (uri: string) => void;
}

function timeAgo(dateStr: string): string {
  const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(dateStr).toLocaleDateString();
}

function authorInitial(post: PostView): string {
  const name = post.author.displayName ?? post.author.handle;
  return name.charAt(0).toUpperCase();
}

export function VideoCard({ post, onOpenAuthor, onOpenPost }: VideoCardProps) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const hlsRef = useRef<Hls | null>(null);
  const [playing, setPlaying] = useState(false);

  const setupVideo = useCallback(() => {
    const video = videoRef.current;
    const playlist = post.embed?.playlist;
    if (!video || !playlist) return;

    const directPlaybackUrl = deriveDirectPlaybackUrl(playlist);

    if (directPlaybackUrl) {
      video.src = directPlaybackUrl;
      return;
    }

    if (playlist.endsWith(".m3u8") && Hls.isSupported()) {
      const hls = new Hls({ enableWorker: true });
      hls.loadSource(playlist);
      hls.attachMedia(video);
      hlsRef.current = hls;
      return;
    }

    // Native HLS (Safari) or direct URL
    video.src = playlist;
  }, [post.embed?.playlist]);

  useEffect(() => {
    return () => {
      hlsRef.current?.destroy();
      hlsRef.current = null;
    };
  }, [post.embed?.playlist]);

  const handlePlay = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    if (!videoRef.current) return;

    setupVideo();
    setPlaying(true);

    videoRef.current.play().catch(() => {
      // autoplay blocked
    });
  }, [setupVideo]);

  const truncatedText = post.text
    ? post.text.length > 100
      ? post.text.slice(0, 97) + "..."
      : post.text
    : null;

  return (
    <article className="video-card" onClick={() => onOpenPost(post.uri)}>
      <div className="video-shell">
        {post.embed ? (
          <>
            <video
              controls={playing}
              muted
              playsInline
              ref={videoRef}
            />
            {post.embed.thumbnail && !playing ? (
              <img
                alt={post.embed.alt ?? ""}
                className="video-thumbnail"
                src={post.embed.thumbnail}
              />
            ) : null}
            {!playing ? (
              <button
                className="video-play-btn"
                onClick={handlePlay}
                type="button"
                aria-label="Play video"
              >
                <span className="play-icon">
                  <span className="play-triangle" />
                </span>
              </button>
            ) : null}
          </>
        ) : (
          <div className="video-empty">
            <div className="video-empty-icon">&#9654;</div>
            No video available
          </div>
        )}
      </div>

      <div className="card-meta">
        <button
          className="card-title"
          onClick={(e) => {
            e.stopPropagation();
            onOpenPost(post.uri);
          }}
          type="button"
        >
          {truncatedText ?? "Untitled post"}
        </button>

        <div className="card-author-row">
          {post.author.avatar ? (
            <img
              alt=""
              className="card-avatar"
              src={post.author.avatar}
            />
          ) : (
            <div className="card-avatar-placeholder">
              {authorInitial(post)}
            </div>
          )}
          <button
            className="card-author-name"
            onClick={(e) => {
              e.stopPropagation();
              onOpenAuthor(post.author.did);
            }}
            type="button"
          >
            {post.author.displayName ?? post.author.handle}
          </button>
          <span className="card-timestamp">{timeAgo(post.createdAt)}</span>
        </div>
      </div>
    </article>
  );
}
