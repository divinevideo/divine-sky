import { startTransition, useEffect, useMemo, useState } from "react";

import {
  fetchAuthorFeed,
  fetchFeedPosts,
  fetchPostThread,
  fetchProfile,
  searchPosts,
  type FeedKind,
  type PostView,
  type ProfileView,
} from "./api";
import { AuthorPage } from "./components/AuthorPage";
import { FeedSwitcher } from "./components/FeedSwitcher";
import { PostDetail } from "./components/PostDetail";
import { SearchBar } from "./components/SearchBar";
import { VideoCard } from "./components/VideoCard";

type Route =
  | { kind: "feed"; feed: FeedKind }
  | { kind: "author"; actor: string }
  | { kind: "post"; uri: string }
  | { kind: "search"; query: string };

function parseHash(hash: string): Route {
  const value = hash.replace(/^#/, "");
  if (!value) {
    return { kind: "feed", feed: "latest" };
  }

  const [kind, rawPayload] = value.split("/", 2);
  const payload = rawPayload ? decodeURIComponent(rawPayload) : "";

  switch (kind) {
    case "trending":
      return { kind: "feed", feed: "trending" };
    case "author":
      return { kind: "author", actor: payload };
    case "post":
      return { kind: "post", uri: payload };
    case "search":
      return { kind: "search", query: payload };
    case "latest":
    default:
      return { kind: "feed", feed: "latest" };
  }
}

function navigate(route: Route) {
  switch (route.kind) {
    case "feed":
      window.location.hash = route.feed;
      break;
    case "author":
      window.location.hash = `author/${encodeURIComponent(route.actor)}`;
      break;
    case "post":
      window.location.hash = `post/${encodeURIComponent(route.uri)}`;
      break;
    case "search":
      window.location.hash = `search/${encodeURIComponent(route.query)}`;
      break;
  }
}

export default function App() {
  const [route, setRoute] = useState<Route>(() => parseHash(window.location.hash));
  const [posts, setPosts] = useState<PostView[]>([]);
  const [profile, setProfile] = useState<ProfileView | null>(null);
  const [focusedPost, setFocusedPost] = useState<PostView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    function onHashChange() {
      startTransition(() => {
        setRoute(parseHash(window.location.hash));
      });
    }

    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    setFocusedPost(null);
    setProfile(null);

    const run = async () => {
      try {
        if (route.kind === "feed") {
          setPosts(await fetchFeedPosts(route.feed, controller.signal));
          return;
        }

        if (route.kind === "author") {
          const [profileValue, authorPosts] = await Promise.all([
            fetchProfile(route.actor, controller.signal),
            fetchAuthorFeed(route.actor, controller.signal),
          ]);
          setProfile(profileValue);
          setPosts(authorPosts);
          return;
        }

        if (route.kind === "post") {
          const post = await fetchPostThread(route.uri, controller.signal);
          setFocusedPost(post);
          setPosts([post]);
          return;
        }

        setPosts(await searchPosts(route.query, controller.signal));
      } catch (reason) {
        if ((reason as Error).name !== "AbortError") {
          setError((reason as Error).message);
        }
      } finally {
        if (!controller.signal.aborted) {
          setLoading(false);
        }
      }
    };

    void run();
    return () => controller.abort();
  }, [route]);

  const title = useMemo(() => {
    switch (route.kind) {
      case "feed":
        return route.feed === "latest" ? "Latest" : "Trending";
      case "author":
        return profile?.displayName ?? profile?.handle ?? route.actor;
      case "post":
        return "Post";
      case "search":
        return `Search: ${route.query}`;
    }
  }, [profile, route]);

  return (
    <div className="shell">
      <div className="bg-glow" />

      <nav className="navbar">
        <button
          className="nav-brand"
          onClick={() => navigate({ kind: "feed", feed: "latest" })}
          type="button"
        >
          <div className="nav-logo">DS</div>
          <span className="nav-title">Divine Sky</span>
        </button>

        <div className="nav-actions">
          <FeedSwitcher
            activeFeed={route.kind === "feed" ? route.feed : "latest"}
            onSelect={(feed) => navigate({ kind: "feed", feed })}
          />
        </div>

        <SearchBar onSearch={(query) => navigate({ kind: "search", query })} />
      </nav>

      <div className="page-header">
        <h1>{title}</h1>
        {route.kind === "search" ? null : (
          <p className="page-subtitle">
            {route.kind === "feed" && route.feed === "latest"
              ? "Recent video posts from the Divine network"
              : route.kind === "feed" && route.feed === "trending"
                ? "Popular videos trending now"
                : null}
          </p>
        )}
      </div>

      {loading ? (
        <div className="status">
          <div className="loading-spinner" />
          <p>Loading...</p>
        </div>
      ) : null}

      {error ? <p className="status status-error">{error}</p> : null}

      {!loading && !error && route.kind === "author" && profile ? (
        <AuthorPage
          onOpenAuthor={(actor) => navigate({ kind: "author", actor })}
          onOpenPost={(uri) => navigate({ kind: "post", uri })}
          posts={posts}
          profile={profile}
        />
      ) : null}

      {!loading && !error && route.kind === "post" && focusedPost ? (
        <PostDetail
          onOpenAuthor={(actor) => navigate({ kind: "author", actor })}
          post={focusedPost}
        />
      ) : null}

      {!loading && !error && route.kind !== "author" && route.kind !== "post" ? (
        <section className="grid">
          {posts.map((post) => (
            <VideoCard
              key={post.uri}
              onOpenAuthor={(actor) => navigate({ kind: "author", actor })}
              onOpenPost={(uri) => navigate({ kind: "post", uri })}
              post={post}
            />
          ))}
          {posts.length === 0 ? (
            <div className="empty-panel">
              <div className="empty-icon">&#9654;</div>
              <p>No videos available for this view yet.</p>
            </div>
          ) : null}
        </section>
      ) : null}
    </div>
  );
}
