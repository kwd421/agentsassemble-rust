import { BarChart3, type LucideIcon } from "lucide-react";


export type ComposerCommand = {
  id: "vote";
  command: string;
  label: string;
  description: string;
};

export const COMPOSER_COMMANDS: ComposerCommand[] = [
  {
    id: "vote",
    command: "/vote",
    label: "투표 만들기",
    description: "질문, 선택지, 투표 시간을 설정해요.",
  },
];

export function matchingComposerCommands(message: string): ComposerCommand[] {
  const match = message.match(/^\s*(\/[^\s]*)$/);
  if (!match) return [];
  const query = match[1].toLocaleLowerCase();
  return COMPOSER_COMMANDS.filter((item) =>
    `${item.command} ${item.label}`.toLocaleLowerCase().includes(query)
  );
}

type MenuItem = { id: string; label: string; command?: string; description?: string; icon?: LucideIcon; disabled?: boolean };

export default function ComposerCommandMenu<T extends MenuItem>({
  listId,
  commands,
  activeIndex,
  onActiveIndexChange,
  onSelect,
  actions = false,
}: {
  listId: string;
  commands: T[];
  activeIndex: number;
  onActiveIndexChange: (index: number) => void;
  onSelect: (command: T) => void;
  actions?: boolean;
}) {
  return (
    <div className="dc-composer-command-menu" aria-label={actions ? "채팅 도구" : "채팅 명령"} role={actions ? "menu" : "listbox"} id={listId}>
      {!actions && <small>명령</small>}
      {commands.map((item, index) => {
        const Icon = item.icon || BarChart3;
        return (
          <button
            key={item.id}
            id={`${listId}-option-${index}`}
            type="button"
            role={actions ? "menuitem" : "option"}
            disabled={item.disabled}
            aria-selected={actions ? undefined : index === activeIndex}
            data-active={index === activeIndex}
            onFocus={() => onActiveIndexChange(index)}
            onMouseDown={(event) => event.preventDefault()}
            onMouseEnter={() => onActiveIndexChange(index)}
            onClick={() => onSelect(item)}
          >
            <span className="dc-composer-command-icon" aria-hidden="true">
              <Icon size={17} />
            </span>
            <span className="dc-composer-command-copy">
              {item.command && <strong>{item.command}</strong>}
              <span>{item.label}</span>
              {item.description && <small>{item.description}</small>}
            </span>
          </button>
        );
      })}
    </div>
  );
}
