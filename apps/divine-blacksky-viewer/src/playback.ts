export function deriveDirectPlaybackUrl(playlistUrl: string): string | null {
  try {
    const url = new URL(playlistUrl);
    if (!url.pathname.startsWith("/playlists/") || !url.pathname.endsWith(".m3u8")) {
      return null;
    }

    url.pathname = url.pathname
      .replace(/^\/playlists\//, "/streams/")
      .replace(/\.m3u8$/, ".mp4");
    url.search = "";
    url.hash = "";
    return url.toString();
  } catch {
    return null;
  }
}
