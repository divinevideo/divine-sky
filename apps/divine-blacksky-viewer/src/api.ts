export type FeedKind = "latest" | "trending";

export interface ProfileView {
  did: string;
  handle: string;
  displayName?: string | null;
  description?: string | null;
  avatar?: string | null;
  banner?: string | null;
}

export interface VideoEmbedView {
  $type: string;
  cid: string;
  playlist: string;
  thumbnail?: string | null;
  alt?: string | null;
}

export interface PostView {
  uri: string;
  cid?: string | null;
  author: ProfileView;
  text: string;
  createdAt: string;
  embed?: VideoEmbedView | null;
}

interface FeedSkeletonResponse {
  feed: Array<{ post: string }>;
}

interface PostsResponse {
  posts: PostView[];
}

interface SearchPostsResponse {
  posts: PostView[];
}

interface AuthorFeedResponse {
  feed: Array<{ post: PostView }>;
}

interface PostThreadResponse {
  thread: {
    post: PostView;
  };
}

const APPVIEW_BASE_URL =
  import.meta.env.VITE_APPVIEW_BASE_URL ?? "http://127.0.0.1:3004";
const FEEDGEN_BASE_URL =
  import.meta.env.VITE_FEEDGEN_BASE_URL ?? "http://127.0.0.1:3002";

const FEED_URIS: Record<FeedKind, string> = {
  latest: "at://did:plc:divine.feed/app.bsky.feed.generator/latest",
  trending: "at://did:plc:divine.feed/app.bsky.feed.generator/trending",
};

async function fetchJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(url, { signal });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return (await response.json()) as T;
}

export async function fetchFeedPosts(
  kind: FeedKind,
  signal?: AbortSignal,
): Promise<PostView[]> {
  const skeletonParams = new URLSearchParams({
    feed: FEED_URIS[kind],
    limit: "12",
  });
  const skeleton = await fetchJson<FeedSkeletonResponse>(
    `${FEEDGEN_BASE_URL}/xrpc/app.bsky.feed.getFeedSkeleton?${skeletonParams.toString()}`,
    signal,
  );

  if (skeleton.feed.length === 0) {
    return [];
  }

  const postsParams = new URLSearchParams();
  skeleton.feed.forEach((item) => postsParams.append("uris", item.post));

  const posts = await fetchJson<PostsResponse>(
    `${APPVIEW_BASE_URL}/xrpc/app.bsky.feed.getPosts?${postsParams.toString()}`,
    signal,
  );
  return posts.posts;
}

export async function fetchProfile(
  actor: string,
  signal?: AbortSignal,
): Promise<ProfileView> {
  const params = new URLSearchParams({ actor });
  return fetchJson<ProfileView>(
    `${APPVIEW_BASE_URL}/xrpc/app.bsky.actor.getProfile?${params.toString()}`,
    signal,
  );
}

export async function fetchAuthorFeed(
  actor: string,
  signal?: AbortSignal,
): Promise<PostView[]> {
  const params = new URLSearchParams({ actor, limit: "12" });
  const response = await fetchJson<AuthorFeedResponse>(
    `${APPVIEW_BASE_URL}/xrpc/app.bsky.feed.getAuthorFeed?${params.toString()}`,
    signal,
  );
  return response.feed.map((entry) => entry.post);
}

export async function fetchPostThread(
  uri: string,
  signal?: AbortSignal,
): Promise<PostView> {
  const params = new URLSearchParams({ uri });
  const response = await fetchJson<PostThreadResponse>(
    `${APPVIEW_BASE_URL}/xrpc/app.bsky.feed.getPostThread?${params.toString()}`,
    signal,
  );
  return response.thread.post;
}

export async function searchPosts(
  query: string,
  signal?: AbortSignal,
): Promise<PostView[]> {
  const params = new URLSearchParams({ q: query, limit: "12" });
  const response = await fetchJson<SearchPostsResponse>(
    `${APPVIEW_BASE_URL}/xrpc/app.bsky.feed.searchPosts?${params.toString()}`,
    signal,
  );
  return response.posts;
}
