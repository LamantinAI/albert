import { Link } from "react-router-dom";

export function Brand() {
  return (
    <Link to="/" className="brand" aria-label="Albert home">
      <span className="brand-mark" />
      <span>
        <span className="brand-word">Albert</span>
        <span className="brand-caption">A place to think</span>
      </span>
    </Link>
  );
}
