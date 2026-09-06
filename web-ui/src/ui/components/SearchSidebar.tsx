import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { AppRoute } from "@/domain/navigation/route";
import { WebUiPhase } from "@/plan/phases";

export const SearchSidebar = () => {
  const navigate = useNavigate();
  const [query, setQuery] = useState("");

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = query.trim();
    const path = AppRoute.toPath(AppRoute.search());
    if (!trimmed) {
      navigate(path);
      return;
    }
    navigate(`${path}?q=${encodeURIComponent(trimmed)}`);
  };

  return (
    <section className="app-card search-sidebar" data-phase={WebUiPhase.notificationsSearch}>
      <h2>検索</h2>
      <form className="search-sidebar-form" onSubmit={handleSubmit}>
        <input
          className="search-input"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="アカウント、投稿、ハッシュタグ"
          autoComplete="off"
          aria-label="検索"
        />
        <button type="submit" className="app-button">
          検索
        </button>
      </form>
    </section>
  );
};
