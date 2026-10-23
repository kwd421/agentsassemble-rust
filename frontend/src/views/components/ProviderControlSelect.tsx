import { type KeyboardEvent as ReactKeyboardEvent, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { overlayHost } from "../../lib/overlayHost";
import { Check, ChevronDown, ChevronLeft, ChevronRight, Search } from "../../ui/icons";

import type { ProviderControlOption } from "../../roomSocketClient";
import {
  filterProviderControlOptions,
  groupProviderControlOptions,
  isFreeProviderOption,
} from "./providerModelOptions";
import {
  menuHeightCap,
  type MenuPosition,
  useWholeRowMenu,
} from "./providerControlMenuLayout";
import ProviderControlOptionContent, {
  providerControlOptionAccessibleName,
  providerControlOptionEffect,
  providerControlOptionHasDescription,
} from "./ProviderControlOptionContent";
import { useProviderModelDetails } from "./ProviderModelDetailsPopover";

const MENU_ITEMS = ':is([role="option"], [role^="menuitem"])';

export default function ProviderControlSelect({
  label,
  options,
  value,
  disabled = false,
  showSpeedIcon = false,
  onChange,
}: {
  label: string;
  options: ProviderControlOption[];
  value: string;
  disabled?: boolean;
  showSpeedIcon?: boolean;
  onChange: (value: string) => void;
}) {
  const snapMenu = useWholeRowMenu();
  const [open, setOpen] = useState(false);
  const [menuPosition, setMenuPosition] = useState<MenuPosition | null>(null);
  const [activeGroup, setActiveGroup] = useState("");
  const [query, setQuery] = useState("");
  const [freeOnly, setFreeOnly] = useState(false);
  const [visionOnly, setVisionOnly] = useState(false);
  const [reasoningOnly, setReasoningOnly] = useState(false);
  const listboxId = useId();
  const controlRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  // The trigger's viewport position when the menu opened. A scroll that leaves
  // it in place (for example the browser scrolling to a focused field) must not
  // dismiss the menu; a real page or panel scroll moves it.
  const anchorRef = useRef<{ left: number; top: number } | null>(null);
  const selectedOption = options.find((option) => option.value === value);
  const hasOnlyResolvedOption = options.length === 1 && Boolean(selectedOption);
  const loadingOptions = !disabled && !hasOnlyResolvedOption && options.length === 0;
  const controlDisabled = disabled || hasOnlyResolvedOption;
  const [notice, setNotice] = useState("");
  const isModelControl = label === "모델";
  const modelDetails = useProviderModelDetails(isModelControl && open);
  const showModelTools = isModelControl && options.length > 1;
  const hasFreeOptions = options.some(isFreeProviderOption);
  const hasVisionOptions = options.some((option) => option.metadata?.vision === true);
  const hasReasoningOptions = options.some((option) => option.metadata?.reasoning === true);
  const filteredOptions = filterProviderControlOptions(
    label,
    options,
    query,
    freeOnly,
    visionOnly,
    reasoningOnly
  );
  const optionGroups = groupProviderControlOptions(label, filteredOptions);
  const showGroupLabels = !query.trim() && optionGroups.length > 1;
  const activeOptionGroup = showGroupLabels
    ? optionGroups.find((group) => group.label === activeGroup)
    : undefined;

  useEffect(() => {
    if (!open) return;
    const close = (event: Event) => {
      const target = event.target;
      if (
        target instanceof Node &&
        !controlRef.current?.contains(target) &&
        !menuRef.current?.contains(target)
      ) {
        setOpen(false);
        setActiveGroup("");
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (activeGroup) {
          setActiveGroup("");
          return;
        }
        setOpen(false);
        buttonRef.current?.focus();
      }
    };
    const closeOnViewportChange = (event: Event) => {
      const target = event.target;
      if (
        target instanceof Node &&
        menuRef.current?.contains(target)
      ) {
        return;
      }
      const anchor = anchorRef.current;
      const rect = buttonRef.current?.getBoundingClientRect();
      if (anchor && rect && Math.abs(rect.left - anchor.left) < 1 && Math.abs(rect.top - anchor.top) < 1) {
        return;
      }
      setOpen(false);
      setActiveGroup("");
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", closeOnEscape);
    window.addEventListener("resize", closeOnViewportChange);
    window.addEventListener("scroll", closeOnViewportChange, true);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("resize", closeOnViewportChange);
      window.removeEventListener("scroll", closeOnViewportChange, true);
    };
  }, [activeGroup, open]);

  useEffect(() => {
    if (!loadingOptions) setNotice("");
  }, [loadingOptions]);

  useEffect(() => {
    if (controlDisabled) {
      setOpen(false);
      setActiveGroup("");
    }
  }, [controlDisabled]);

  useEffect(() => {
    setActiveGroup("");
  }, [freeOnly, query, reasoningOnly, visionOnly]);

  useLayoutEffect(() => {
    if (!open || !menuPosition || !menuRef.current) return;
    const margin = 8;
    const rect = menuRef.current.getBoundingClientRect();
    const left = Math.min(
      Math.max(margin, menuPosition.left),
      Math.max(margin, window.innerWidth - rect.width - margin)
    );
    const top = Math.min(
      Math.max(margin, menuPosition.top),
      Math.max(margin, window.innerHeight - rect.height - margin)
    );
    if (left !== menuPosition.left || top !== menuPosition.top) {
      setMenuPosition((current) => current && { ...current, left, top });
    }
  }, [activeGroup, filteredOptions.length, freeOnly, menuPosition, open, query, reasoningOnly, visionOnly]);

  useLayoutEffect(() => {
    if (!open || !menuPosition || !menuRef.current) return;
    const menu = menuRef.current;
    const target = !activeGroup && showModelTools
      ? menu.querySelector<HTMLElement>('input[aria-label="모델 검색"]')
      : menu.querySelector<HTMLElement>(`${MENU_ITEMS}[data-selected="true"]`) ??
        menu.querySelector<HTMLElement>(MENU_ITEMS);
    target?.focus({ preventScroll: true });
    // preventScroll keeps the page still; the bounded list still has to show the row.
    if (target && !(target instanceof HTMLInputElement)) target.scrollIntoView?.({ block: "nearest" });
  }, [activeGroup, menuPosition, open, showModelTools]);

  // Arrow keys walk the rows like a native select; Enter and Space press the
  // focused row because every row is a button.
  function moveFocus(event: ReactKeyboardEvent<HTMLDivElement>) {
    const menu = menuRef.current;
    if (!menu || event.nativeEvent.isComposing) return;
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    if (event.target instanceof HTMLInputElement && event.key !== "ArrowDown") return;
    const items = Array.from(menu.querySelectorAll<HTMLButtonElement>(MENU_ITEMS)).filter(
      (item) => !item.disabled
    );
    if (items.length === 0) return;
    event.preventDefault();
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    const search = menu.querySelector<HTMLElement>('input[aria-label="모델 검색"]');
    const next =
      event.key === "Home" ? items[0]
      : event.key === "End" ? items[items.length - 1]
      : event.key === "ArrowDown" ? items[Math.min(current + 1, items.length - 1)]
      : current > 0 ? items[current - 1]
      : search ?? items[0];
    next.focus();
  }

  function toggleMenu() {
    if (controlDisabled || !buttonRef.current) return;
    if (loadingOptions) {
      setNotice(`${label} 목록을 불러오는 중이에요.`);
      return;
    }
    if (open) {
      setOpen(false);
      setActiveGroup("");
      return;
    }
    const rect = buttonRef.current.getBoundingClientRect();
    anchorRef.current = { left: rect.left, top: rect.top };
    const optionHeight =
      !isModelControl && options.some(providerControlOptionHasDescription) ? 50 : 36;
    const estimatedHeight = Math.min(
      menuHeightCap(optionHeight) + (showModelTools ? 90 : 0),
      (showGroupLabels ? optionGroups.length * 36 : filteredOptions.length * optionHeight) +
        (showModelTools ? 90 : 8)
    );
    const spaceBelow = window.innerHeight - rect.bottom - 8;
    const spaceAbove = rect.top - 8;
    const openAbove = spaceBelow < estimatedHeight && spaceAbove > spaceBelow;
    setMenuPosition({
      left: rect.left,
      top: openAbove
        ? Math.max(8, rect.top - estimatedHeight - 6)
        : rect.bottom + 6,
      width: rect.width,
    });
    setOpen(true);
  }

  function selectOption(option: ProviderControlOption) {
    if (option.metadata?.disabled === true) return;
    onChange(option.value);
    setOpen(false);
    setActiveGroup("");
    setQuery("");
    buttonRef.current?.focus({ preventScroll: true });
  }

  return (
    <div className="dc-agent-select" ref={controlRef}>
      <button
        ref={buttonRef}
        type="button"
        className="dc-agent-select-trigger"
        role="combobox"
        aria-label={label}
        aria-controls={listboxId}
        aria-expanded={open}
        aria-haspopup={showGroupLabels ? "menu" : "listbox"}
        aria-disabled={loadingOptions || undefined}
        data-effect={providerControlOptionEffect(selectedOption)}
        disabled={controlDisabled}
        onClick={toggleMenu}
      >
        {selectedOption ? (
          <ProviderControlOptionContent
            option={selectedOption}
            pricingOnly={isModelControl}
            showSpeedIcon={showSpeedIcon}
          />
        ) : (
          <span className="truncate preserve-words">선택 필요</span>
        )}
        <ChevronDown size={15} aria-hidden="true" />
      </button>
      {notice && !open && <p className="dc-agent-select-note" role="status">{notice}</p>}
      {label === "권한" && value === "full_access" && (
        <p className="text-xs text-amber-600 preserve-words">방에 있는 누구의 말이든 이 컴퓨터에서 승인 없이 명령으로 실행될 수 있어요.</p>
      )}
      {open &&
        menuPosition &&
        createPortal(
          // Keep the search field focused while pressing a row. A blur
          // re-render before mouseup would otherwise swallow the first click.
          <div
            ref={menuRef}
            className="dc-agent-select-popover"
            style={menuPosition}
            onKeyDown={moveFocus}
            onMouseDown={(event) => {
              if (event.target instanceof Element && event.target.closest("button")) {
                event.preventDefault();
              }
            }}
          >
              {showModelTools && (
                <div className="dc-agent-model-tools">
                  <label className="dc-search dc-search--compact dc-agent-model-search">
                    <Search size={15} aria-hidden="true" />
                    <input
                      type="search"
                      aria-label="모델 검색"
                      value={query}
                      placeholder={`${options.length.toLocaleString()}개 모델 검색`}
                      onChange={(event) => setQuery(event.currentTarget.value)}
                    />
                  </label>
                  {(hasFreeOptions || hasVisionOptions || hasReasoningOptions) && (
                    <div className="dc-agent-model-filters" aria-label="모델 필터">
                      {hasFreeOptions && (
                        <button
                          type="button"
                          className="dc-agent-model-free-filter"
                          aria-label="무료 모델만 보기"
                          aria-pressed={freeOnly}
                          data-active={freeOnly}
                          onClick={() => setFreeOnly((current) => !current)}
                        >
                          무료
                        </button>
                      )}
                      {hasVisionOptions && (
                        <button
                          type="button"
                          className="dc-agent-model-free-filter"
                          aria-label="비전 모델만 보기"
                          aria-pressed={visionOnly}
                          data-active={visionOnly}
                          onClick={() => setVisionOnly((current) => !current)}
                        >
                          비전
                        </button>
                      )}
                      {hasReasoningOptions && (
                        <button
                          type="button"
                          className="dc-agent-model-free-filter"
                          aria-label="추론 모델만 보기"
                          aria-pressed={reasoningOnly}
                          data-active={reasoningOnly}
                          onClick={() => setReasoningOnly((current) => !current)}
                        >
                          추론
                        </button>
                      )}
                    </div>
                  )}
                </div>
              )}
              {activeOptionGroup && (
                <div className="dc-agent-select-drilldown-header">
                  <button
                    type="button"
                    aria-label="모델 목록으로 돌아가기"
                    onClick={() => setActiveGroup("")}
                  >
                    <ChevronLeft size={15} aria-hidden="true" />
                    <span className="dc-agent-select-level-copy">
                      <small>모델 제공사</small>
                      <strong className="truncate preserve-words">{activeOptionGroup.label} 모델</strong>
                    </span>
                  </button>
                </div>
              )}
              <div
                id={listboxId}
                ref={snapMenu}
                className="dc-agent-select-menu"
                role={showGroupLabels && !activeOptionGroup ? "menu" : "listbox"}
                aria-label={
                  activeOptionGroup
                    ? `${activeOptionGroup.label} 모델`
                    : showGroupLabels
                      ? `${label} 분류`
                      : label
                }
                onScroll={modelDetails.hide}
              >
              {filteredOptions.length === 0 ? (
                <p className="dc-agent-model-empty" role="status">조건에 맞는 모델이 없어요.</p>
              ) : activeOptionGroup
                ? activeOptionGroup.options.map((option) => {
                    const selected = option.value === value;
                    return (
                      <button
                        key={option.value || "default"}
                        type="button"
                        role="option"
                        aria-label={providerControlOptionAccessibleName(option)}
                        aria-selected={selected}
                        disabled={option.metadata?.disabled === true}
                        data-selected={selected}
                        data-effect={providerControlOptionEffect(option)}
                        {...modelDetails.bind(option)}
                        onClick={() => selectOption(option)}
                      >
                        <ProviderControlOptionContent
                          option={option}
                          showDescription={!isModelControl}
                          pricingOnly={isModelControl}
                          showSpeedIcon={showSpeedIcon}
                        />
                        {selected && <Check size={15} aria-hidden="true" />}
                      </button>
                    );
                  })
                : showGroupLabels
                ? optionGroups.map((group) => {
                    if (group.options.length === 1) {
                      const option = group.options[0];
                      const selected = option.value === value;
                      return (
                        <button
                          key={group.label}
                          type="button"
                          role="menuitemradio"
                          aria-label={providerControlOptionAccessibleName(option)}
                          aria-checked={selected}
                          disabled={option.metadata?.disabled === true}
                          data-selected={selected}
                          data-effect={providerControlOptionEffect(option)}
                          {...modelDetails.bind(option)}
                          onClick={() => selectOption(option)}
                        >
                          <ProviderControlOptionContent
                            option={option}
                            showDescription={!isModelControl}
                            pricingOnly={isModelControl}
                            showSpeedIcon={showSpeedIcon}
                          />
                          {selected && <Check size={15} aria-hidden="true" />}
                        </button>
                      );
                    }
                    const containsSelected = group.options.some(
                      (option) => option.value === value
                    );
                    return (
                      <button
                        key={group.label}
                        type="button"
                        role="menuitem"
                        className="dc-agent-select-group"
                        aria-label={`${group.label} 제공사, ${group.options.length.toLocaleString()}개 모델`}
                        aria-haspopup="listbox"
                        aria-expanded={activeGroup === group.label}
                        data-selected={containsSelected}
                        onClick={() => setActiveGroup(group.label)}
                      >
                        <span className="dc-agent-select-group-copy">
                          <span className="truncate preserve-words">{group.label}</span>
                          <small>{group.options.length.toLocaleString()}개 모델</small>
                        </span>
                        <span className="dc-agent-select-group-trailing">
                          <ChevronRight className="dc-agent-select-group-arrow" size={15} aria-hidden="true" />
                        </span>
                      </button>
                    );
                  })
                : filteredOptions.map((option) => {
                  const selected = option.value === value;
                  return (
                    <button
                      key={option.value || "default"}
                      type="button"
                      role="option"
                      aria-label={providerControlOptionAccessibleName(option)}
                      aria-selected={selected}
                      disabled={option.metadata?.disabled === true}
                      data-selected={selected}
                      data-effect={providerControlOptionEffect(option)}
                      {...modelDetails.bind(option)}
                      onClick={() => selectOption(option)}
                    >
                      <ProviderControlOptionContent
                        option={option}
                        showDescription={!isModelControl}
                        pricingOnly={isModelControl}
                        showSpeedIcon={showSpeedIcon}
                      />
                      {selected && <Check size={15} aria-hidden="true" />}
                    </button>
                  );
                })}
              </div>
          </div>,
          overlayHost(controlRef.current)
        )}
      {modelDetails.popover}
    </div>
  );
}
