import type { PostView, ProfileView } from "../api";
import { VideoCard } from "./VideoCard";

interface AuthorPageProps {
  profile: ProfileView;
  posts: PostView[];
  onOpenAuthor: (actor: string) => void;
  onOpenPost: (uri: string) => void;
}

function profileInitial(profile: ProfileView): string {
  const name = profile.displayName ?? profile.handle;
  return name.charAt(0).toUpperCase();
}

export function AuthorPage({
  profile,
  posts,
  onOpenAuthor,
  onOpenPost,
}: AuthorPageProps) {
  return (
    <section className="author-panel">
      <div className="author-hero">
        {profile.banner ? (
          <img alt="" className="author-banner" src={profile.banner} />
        ) : (
          <div className="author-banner-empty" />
        )}
        <div className="author-info">
          {profile.avatar ? (
            <img alt="" className="author-avatar" src={profile.avatar} />
          ) : (
            <div className="author-avatar-placeholder">
              {profileInitial(profile)}
            </div>
          )}
          <div className="author-text">
            <h2>{profile.displayName ?? profile.did}</h2>
            <p className="author-handle">@{profile.handle}</p>
            <p className="author-bio">
              {profile.description ?? "No description available."}
            </p>
          </div>
        </div>
      </div>

      <div className="grid">
        {posts.map((post) => (
          <VideoCard
            key={post.uri}
            onOpenAuthor={onOpenAuthor}
            onOpenPost={onOpenPost}
            post={post}
          />
        ))}
        {posts.length === 0 ? (
          <div className="empty-panel">
            <p>No posts from this author yet.</p>
          </div>
        ) : null}
      </div>
    </section>
  );
}
