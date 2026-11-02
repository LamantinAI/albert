import { Identity } from "../../components/Brand";
import { Icon } from "../../components/Icon";

const suggestions = [
  {
    title: "Think it through",
    prompt: "Help me think through an idea.",
    icon: "chats",
  },
  { title: "Make a plan", prompt: "Help me make a plan.", icon: "models" },
  {
    title: "Explore something",
    prompt: "Help me explore a question.",
    icon: "search",
  },
] as const;

export function ConversationStart({
  temporary = false,
  disabled = false,
  onSuggestion,
  heading = "h1",
}: {
  temporary?: boolean;
  disabled?: boolean;
  onSuggestion: (text: string) => void;
  heading?: "h1" | "h2";
}) {
  const Heading = heading;
  return (
    <div className="welcome-content">
      <Identity className="chat-identity" />
      <div className="welcome-copy">
        <Heading>What are we working on?</Heading>
        <p className="welcome-mode-copy">
          <span aria-hidden={temporary}>
            A question, an idea, something worth figuring out.
          </span>
          <span aria-hidden={!temporary}>
            A temporary conversation. Save it when you want to keep it.
          </span>
        </p>
      </div>
      <div className="welcome-suggestions">
        {suggestions.map((item) => (
          <button
            key={item.title}
            disabled={disabled}
            onClick={() => onSuggestion(item.prompt)}
          >
            <Icon name={item.icon} size={16} />
            <strong>{item.title}</strong>
          </button>
        ))}
      </div>
    </div>
  );
}
