import { FormEvent, useState } from "react";

interface SearchBarProps {
  onSearch: (query: string) => void;
}

export function SearchBar({ onSearch }: SearchBarProps) {
  const [query, setQuery] = useState("");

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = query.trim();
    if (trimmed) {
      onSearch(trimmed);
    }
  }

  return (
    <form className="search-bar" onSubmit={handleSubmit}>
      <input
        aria-label="Search posts"
        className="search-input"
        onChange={(event) => setQuery(event.target.value)}
        placeholder="Search videos..."
        value={query}
      />
      <button className="search-button" type="submit">
        Search
      </button>
    </form>
  );
}
