import { Link } from "react-router";

const ME_LINKS = [
  { to: "/bookmarks", label: "ブックマーク" },
  { to: "/favourites", label: "お気に入り" },
  { to: "/scheduled", label: "予約投稿" },
  { to: "/lists", label: "リスト" },
  { to: "/settings", label: "設定" },
] as const;

export const MeHubNav = () => (
  <nav className="me-hub-nav" aria-label="自分">
    {ME_LINKS.map((item) => (
      <Link key={item.to} className="app-button app-button-secondary" to={item.to}>
        {item.label}
      </Link>
    ))}
  </nav>
);

export const MeBackLink = () => (
  <p className="thread-back">
    <Link to="/profile">← 自分</Link>
  </p>
);
