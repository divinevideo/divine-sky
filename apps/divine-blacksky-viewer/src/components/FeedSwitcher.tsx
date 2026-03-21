import type { FeedKind } from "../api";

interface FeedSwitcherProps {
  activeFeed: FeedKind;
  onSelect: (feed: FeedKind) => void;
}

export function FeedSwitcher({ activeFeed, onSelect }: FeedSwitcherProps) {
  return (
    <div className="feed-switcher">
      {(["latest", "trending"] as FeedKind[]).map((feed) => (
        <button
          key={feed}
          className={activeFeed === feed ? "chip chip-active" : "chip"}
          onClick={() => onSelect(feed)}
          type="button"
        >
          {feed === "latest" ? "Latest" : "Trending"}
        </button>
      ))}
    </div>
  );
}
