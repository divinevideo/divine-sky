import { describe, expect, it } from "vitest";

import { deriveDirectPlaybackUrl } from "./playback";

describe("deriveDirectPlaybackUrl", () => {
  it("rewrites local media-view playlists into direct mp4 stream URLs", () => {
    expect(
      deriveDirectPlaybackUrl(
        "http://127.0.0.1:3100/playlists/did/plc/divineblackskyapplab/bafkrei123.m3u8",
      ),
    ).toBe(
      "http://127.0.0.1:3100/streams/did/plc/divineblackskyapplab/bafkrei123.mp4",
    );
  });

  it("returns null for non-playlist URLs", () => {
    expect(
      deriveDirectPlaybackUrl(
        "http://127.0.0.1:3100/thumbnails/did/plc/divineblackskyapplab/bafkrei123.jpg",
      ),
    ).toBeNull();
  });
});
