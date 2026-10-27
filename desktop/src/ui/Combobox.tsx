import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { ChevronDown, Plus } from "lucide-react";
import { Popover } from "./Popover";
import { t, useLanguage } from "../i18n";

/** The id of the "Create “…”" row that `create` adds. */
const CREATE = "\u0000create";

export type Option = {
  id: string;
  label: string;
  /** Heading the option is listed under. */
  group?: string;
  /** Right-aligned text, e.g. an available balance. */
  detail?: ReactNode;
  detailTone?: string;
  /** Extra words that match a search but are not shown. */
  keywords?: string;
  disabled?: boolean;
};
const LIMIT = 200;

/**
 * A text field with a filtered list. In select mode `value` is an option id; in free-text
 * mode (`freeText`) the field's text is the value and options are suggestions.
 */
export function Combobox({
  options,
  value,
  onChange,
  freeText = false,
  text,
  onTextChange,
  label,
  placeholder,
  autoFocus,
  className = "",
  emptyText = t("No matches"),
  onKeyDown,
  disabled,
  groupHints = true,
  create,
  onClear,
}: {
  options: Option[];
  /** Selected option id (select mode). */
  value?: string | null;
  onChange: (id: string, option: Option) => void;
  freeText?: boolean;
  /** Current text (free-text mode). */
  text?: string;
  onTextChange?: (text: string) => void;
  label: string;
  placeholder?: string;
  autoFocus?: boolean;
  className?: string;
  emptyText?: string;
  /** Called for keys the combobox does not handle (Enter when closed, Escape when closed…). */
  onKeyDown?: (e: KeyboardEvent<HTMLInputElement>) => void;
  disabled?: boolean;
  /** While searching, show each match's group beside it (e.g. a category's group). */
  groupHints?: boolean;
  /**
   * Offer to create what was typed when no option has that name: a row pinned below the
   * matches, "Create category “Pet food”", that calls `onCreate` with the text instead of
   * choosing an option. With `blank`, the row is there before anything is typed too, and
   * calls `onCreate("")`. In free-text mode the row is highlighted unless an option starts
   * with the text (or a word in it does), so Enter and Tab keep a new name as typed.
   */
  create?: { label: string; blank?: string; onCreate: (text: string) => void };
  /** Select mode: emptying the field and leaving it (or pressing Enter) chooses nothing, instead
   * of putting the choice back. */
  onClear?: () => void;
}) {
  const id = useId();
  const box = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const selected = options.find((o) => o.id === value);
  const shown = freeText ? (text ?? "") : (query ?? selected?.label ?? "");
  const search = (freeText ? (text ?? "") : (query ?? "")).trim().toLowerCase();
  // The field was emptied by hand: with onClear, that's a choice of nothing.
  const emptied = !!onClear && !freeText && query !== null && !query.trim();

  const typed = (freeText ? (text ?? "") : (query ?? "")).trim();
  const language = useLanguage();
  const { matches, strong } = useMemo(() => {
    let found: Option[],
      strong = 0;
    if (!search) found = options.filter((o) => !o.disabled).slice(0, LIMIT);
    else {
      const starts: Option[] = [],
        words: Option[] = [],
        contains: Option[] = [];
      for (const o of options) {
        if (o.disabled) continue;
        const name = o.label.toLowerCase();
        if (name.startsWith(search)) starts.push(o);
        else if (name.includes(` ${search}`)) words.push(o);
        else if (
          name.includes(search) ||
          o.group?.toLowerCase().includes(search) ||
          o.keywords?.toLowerCase().includes(search)
        )
          contains.push(o);
        if (starts.length >= LIMIT) break;
      }
      // Keep group order, but prefer names that start with the search, then a word that does.
      found = [...starts, ...words, ...contains].slice(0, LIMIT);
      strong = Math.min(found.length, starts.length + words.length);
    }
    const exact = !!search && options.some((o) => o.label.toLowerCase() === search);
    const row = search && !exact ? t("{action} “{name}”", { action: create?.label ?? "", name: typed }) : create?.blank;
    return { matches: create && row ? [...found, { id: CREATE, label: row }] : found, strong };
  }, [options, search, create, typed, language]);
  const grouped = !search && options.some((o) => o.group);
  const creating = matches.at(-1)?.id === CREATE;

  useEffect(() => {
    const index = matches.findIndex((o) => o.id === value);
    // Typing a new name keeps it: nothing starts like it, so the create row comes first.
    if (freeText && search && creating && !strong) setActive(matches.length - 1);
    // Leave no row highlighted for a pending clear. Arrow keys or the pointer can
    // still highlight a category explicitly before Enter chooses it.
    else setActive(emptied ? -1 : index >= 0 && !search ? index : 0);
  }, [matches, value, search, freeText, creating, strong, emptied]);
  useEffect(() => {
    list.current
      ?.querySelector<HTMLElement>(`[data-index="${active}"]`)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [active, open]);

  const choose = (option: Option | undefined) => {
    if (!option) return;
    setOpen(false);
    setQuery(null);
    if (option.id === CREATE) return create?.onCreate(option.label === create.blank ? "" : typed);
    if (freeText) onTextChange?.(option.label);
    onChange(option.id, option);
  };
  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) return setOpen(true);
      const step = e.key === "ArrowDown" ? 1 : -1;
      setActive((i) => Math.min(Math.max(0, i + step), matches.length - 1));
      return;
    }
    if (e.key === "Enter" && emptied && (!open || !matches[active])) {
      // Clearing is a selection, just like choosing a row. Let the parent receive the
      // next Enter after it has rendered the new value, rather than saving stale state.
      e.preventDefault();
      setOpen(false);
      setQuery(null);
      onClear!();
      return;
    }
    if (open && e.key === "Enter" && (matches[active] && (!freeText || search))) {
      // Keeping typed text chooses nothing, so Enter carries on as it would with no list.
      if (freeText && matches[active].id === CREATE) {
        setOpen(false);
        onKeyDown?.(e);
        return;
      }
      e.preventDefault();
      choose(matches[active]);
      return;
    }
    if (open && e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
      setQuery(null);
      return;
    }
    // Tab takes the highlighted match, but never creates anything on its way to the next field.
    if (open && e.key === "Tab" && (search || emptied) && matches[active] && matches[active].id !== CREATE) choose(matches[active]);
    onKeyDown?.(e);
  };

  let lastGroup: string | undefined;
  return (
    <div className={`combobox ${className}`} ref={box}>
      <input
        role="combobox"
        aria-label={label}
        aria-expanded={open}
        aria-controls={`${id}-list`}
        aria-autocomplete="list"
        aria-activedescendant={open && matches[active] ? `${id}-${active}` : undefined}
        placeholder={placeholder ?? (freeText ? undefined : selected ? undefined : t("Choose…"))}
        value={shown}
        autoFocus={autoFocus}
        disabled={disabled}
        onFocus={(e) => {
          if (!freeText) e.currentTarget.select();
        }}
        onClick={() => setOpen(true)}
        onChange={(e) => {
          if (freeText) onTextChange?.(e.target.value);
          else {
            setQuery(e.target.value);
            if (onClear && !e.target.value.trim()) setActive(-1);
          }
          setOpen(true);
        }}
        onBlur={() => {
          if (emptied) onClear!();
          setOpen(false);
          setQuery(null);
        }}
        onKeyDown={key}
      />
      {!freeText && (
        <ChevronDown
          size={14}
          className="combobox-caret"
          aria-hidden
          onMouseDown={(e) => {
            e.preventDefault();
            box.current?.querySelector("input")?.focus();
            setOpen((o) => !o);
          }}
        />
      )}
      <Popover
        open={open && (matches.length > 0 || !freeText)}
        onClose={(reason) => {
          // Outside mousedown closes the list before the input's blur can commit the
          // empty value. Escape and Back cancel the edit instead.
          if (reason === "outside" && emptied) onClear!();
          setOpen(false);
          setQuery(null);
        }}
        anchor={box}
        matchWidth
        focus={false}
        role="presentation"
        className="combobox-popover"
      >
        <ul id={`${id}-list`} role="listbox" aria-label={label} ref={list} onMouseDown={(e) => e.preventDefault()}>
          {matches.map((o, i) => {
            const heading = grouped && o.group !== lastGroup ? o.group : undefined;
            lastGroup = o.group;
            return [
              heading && (
                <li key={`g-${heading}-${i}`} className="combobox-group" role="presentation">
                  {heading}
                </li>
              ),
              <li
                key={o.id}
                id={`${id}-${i}`}
                data-index={i}
                role="option"
                aria-selected={o.id === value}
                className={`combobox-option${i === active ? " active" : ""}${o.id === value ? " selected" : ""}${o.id === CREATE ? " create" : ""}`}
                onMouseEnter={() => setActive(i)}
                onClick={() => choose(o)}
              >
                {o.id === CREATE && <Plus size={14} aria-hidden />}
                <span>{o.label}</span>
                {!grouped && search && groupHints && o.group && <small>{o.group}</small>}
                {o.detail !== undefined && <span className={`combobox-detail ${o.detailTone ?? ""}`}>{o.detail}</span>}
              </li>,
            ];
          })}
          {!matches.length && <li className="combobox-empty">{emptyText}</li>}
        </ul>
      </Popover>
    </div>
  );
}
