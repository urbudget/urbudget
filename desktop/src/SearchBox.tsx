import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Search, X } from "lucide-react";
import { Popover } from "./ui";
import { payeeIndex } from "./payees";
import { sameTerm, suggest, termLabel, type Suggestion, type Term } from "./search";
import type { Budget } from "./types";
import { getDirection, t, useLanguage } from "./i18n";

/**
 * The register's search bar. Filters show as chips; typing offers matching payees, categories,
 * accounts, amounts and states. Enter adds the highlighted one (by default, a text search),
 * Backspace in an empty field removes the last chip. It's one line, compact until it's used: it
 * widens when first clicked and stays wide, its chips scrolling sideways.
 */
export function SearchBox({
  budget,
  text,
  terms,
  onText,
  onTerms,
}: {
  budget: Budget;
  text: string;
  terms: Term[];
  onText: (text: string) => void;
  onTerms: (terms: Term[]) => void;
}) {
  const id = useId();
  const box = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [focused, setFocused] = useState(false);
  // Once clicked it stays wide: shrinking as you click away would slide the buttons after it out
  // from under the pointer.
  const [used, setUsed] = useState(false);
  // Which ends of the chips are scrolled out of view, to fade them.
  const [hidden, setHidden] = useState({ start: false, end: false });
  const [active, setActive] = useState(0);
  const expanded = used || terms.length > 0 || !!text;
  const payees = useMemo(() => payeeIndex(budget).map((p) => p.name), [budget.transactions, budget.payees]);
  // Suggestions' labels are in the language in use.
  const language = useLanguage();
  const suggestions = useMemo(
    () => suggest(text, budget, payees).filter((s) => !s.term || !terms.some((term) => sameTerm(term, s.term!))),
    [text, budget.categories, budget.accounts, payees, terms, language],
  );
  useEffect(() => setActive(0), [text]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView?.({ block: "nearest" });
  }, [active]);
  const measure = () => {
    const s = scroller.current;
    // Right to left, scrollLeft runs from 0 at the start towards negative values.
    const scrolled = s ? Math.abs(s.scrollLeft) : 0;
    if (s) setHidden({ start: scrolled > 0, end: scrolled + s.clientWidth < s.scrollWidth - 1 });
  };
  // While you type, the end (where you type) is in view; otherwise the first chips are.
  useEffect(() => {
    const s = scroller.current;
    if (!s) return;
    s.scrollLeft = focused ? (getDirection() === "rtl" ? -s.scrollWidth : s.scrollWidth) : 0;
    measure();
  }, [terms.length, focused]);
  useEffect(() => {
    if (typeof ResizeObserver === "undefined" || !scroller.current) return;
    const observer = new ResizeObserver(measure);
    observer.observe(scroller.current);
    return () => observer.disconnect();
  }, []);
  const add = (term: Term) => {
    onTerms([...terms, term]);
    onText("");
  };
  const choose = (s: Suggestion | undefined) => {
    if (!s) return;
    if (s.term) add(s.term);
    else if (s.prefix) onText(s.prefix);
    input.current?.focus();
  };
  const keys = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) return setOpen(true);
      setActive((i) => Math.min(Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)), suggestions.length - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (open && suggestions[active]) choose(suggestions[active]);
      else if (text.trim()) add({ field: "text", value: text.trim() });
    } else if (e.key === "Escape") {
      if (open) {
        e.preventDefault();
        setOpen(false);
      } else if (text) {
        e.preventDefault();
        onText("");
      }
    } else if (e.key === "Backspace" && !text && terms.length) {
      e.preventDefault();
      onTerms(terms.slice(0, -1));
    }
  };
  return (
    <div
      className={`search-box${expanded ? " expanded" : ""}`}
      ref={box}
      onMouseDown={(e) => {
        // A press anywhere but the field or a chip's ✕ (the magnifier, a chip's words) types in it.
        if (!(e.target as Element).closest("input, button")) {
          e.preventDefault();
          input.current?.focus();
        }
      }}
    >
      <Search size={15} aria-hidden />
      <div
        className={`search-terms${hidden.start ? " hidden-start" : ""}${hidden.end ? " hidden-end" : ""}`}
        ref={scroller}
        onScroll={measure}
      >
        {terms.map((term, i) => (
          <span className={`chip chip-${term.field}`} key={`${i}:${termLabel(term, budget)}`}>
            {termLabel(term, budget)}
            <button
              className="chip-remove"
              aria-label={t("Remove {filter}", { filter: termLabel(term, budget) })}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => onTerms(terms.filter((_, j) => j !== i))}
            >
              <X size={12} />
            </button>
          </span>
        ))}
        <input
          ref={input}
          role="combobox"
          aria-label={t("Search transactions")}
          aria-expanded={open}
          aria-controls={`${id}-list`}
          aria-autocomplete="list"
          aria-activedescendant={open && suggestions[active] ? `${id}-${active}` : undefined}
          placeholder={terms.length ? t("Add a filter…") : focused ? t("Search, or type payee: category: amount: is:") : t("Search")}
          value={text}
          onChange={(e) => {
            onText(e.target.value);
            setOpen(true);
          }}
          onFocus={() => {
            setFocused(true);
            setUsed(true);
            setOpen(true);
          }}
          onClick={() => setOpen(true)}
          onBlur={() => {
            setFocused(false);
            setOpen(false);
          }}
          onKeyDown={keys}
        />
      </div>
      <Popover
        open={open && suggestions.length > 0}
        onClose={() => setOpen(false)}
        anchor={box}
        matchWidth
        focus={false}
        role="presentation"
        className="search-suggestions"
      >
        <ul id={`${id}-list`} role="listbox" aria-label={t("Search suggestions")} ref={list} onMouseDown={(e) => e.preventDefault()}>
          {!text.trim() && <li className="search-hint">{t("Filter by…")}</li>}
          {suggestions.map((s, i) => (
            <li
              key={`${i}:${s.label}`}
              id={`${id}-${i}`}
              data-index={i}
              role="option"
              aria-selected={i === active}
              className={i === active ? "active" : ""}
              onMouseEnter={() => setActive(i)}
              onClick={() => choose(s)}
            >
              <span>{s.label}</span>
              {s.hint && <kbd>{s.hint}</kbd>}
            </li>
          ))}
        </ul>
      </Popover>
    </div>
  );
}
