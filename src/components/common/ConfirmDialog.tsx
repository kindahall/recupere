import { useCallback, useEffect, useId, useRef } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  description: string;
  confirmLabel?: string;
  cancelLabel?: string;
  variant?: 'danger' | 'warning' | 'default';
  onConfirm: () => void;
  onCancel: () => void;
}

// WCAG 2.4.3 + 2.1.2 compliant modal:
//   - `role="dialog"` + `aria-modal="true"` tell screen readers to treat the
//     rest of the page as inert,
//   - `aria-labelledby` / `aria-describedby` announce the title + body,
//   - a Tab/Shift+Tab focus trap prevents the keyboard from escaping the
//     dialog (the background is visually there, but logically it's not
//     reachable),
//   - Escape closes (parity with native `<dialog>`),
//   - focus is restored to the element that owned focus before the dialog
//     opened, so the user isn't dropped onto `<body>` after confirming.
//
// Focus lands on the CANCEL button by default: destructive actions should
// require an explicit second intent, not a Tab+Enter autopilot path.
export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  cancelLabel,
  variant = 'default',
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  const contentRef = useRef<HTMLDialogElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const descriptionId = useId();

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!open) return;

      if (e.key === 'Escape') {
        e.preventDefault();
        onCancel();
        return;
      }

      if (e.key === 'Tab') {
        const focusables: HTMLButtonElement[] = [];
        if (cancelRef.current) focusables.push(cancelRef.current);
        if (confirmRef.current) focusables.push(confirmRef.current);
        if (focusables.length === 0) return;

        const first = focusables[0];
        const last = focusables[focusables.length - 1];
        const active = document.activeElement as HTMLElement | null;

        if (e.shiftKey && active === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && active === last) {
          e.preventDefault();
          first.focus();
        } else if (active && !focusables.includes(active as HTMLButtonElement)) {
          // Focus escaped the trap (e.g. browser autofill popup) — pull it
          // back to the cancel button, which is the safe default.
          e.preventDefault();
          first.focus();
        }
      }
    },
    [onCancel, open],
  );

  useEffect(() => {
    if (!open) return;
    previouslyFocusedRef.current = document.activeElement as HTMLElement | null;
    document.addEventListener('keydown', handleKeyDown);
    // Defer focus by one tick so the portal DOM is attached first.
    const raf = window.requestAnimationFrame(() => {
      cancelRef.current?.focus();
    });
    return () => {
      window.cancelAnimationFrame(raf);
      document.removeEventListener('keydown', handleKeyDown);
      const previous = previouslyFocusedRef.current;
      if (previous && typeof previous.focus === 'function') {
        previous.focus();
      }
    };
  }, [open, handleKeyDown]);

  if (!open) return null;

  const confirmBtnClass = variant === 'danger' ? 'btn btn-danger' : 'btn btn-primary';

  return createPortal(
    <div
      className="dialog-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onCancel();
        }
      }}
    >
      <dialog
        open
        ref={contentRef}
        className="dialog-content"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div id={titleId} className="dialog-title">
          {title}
        </div>
        <div id={descriptionId} className="dialog-desc">
          {description}
        </div>
        <div className="dialog-actions">
          <button type="button" ref={cancelRef} className="btn btn-secondary" onClick={onCancel}>
            {cancelLabel ?? t('common.cancel')}
          </button>
          <button type="button" ref={confirmRef} className={confirmBtnClass} onClick={onConfirm}>
            {confirmLabel ?? t('common.confirm')}
          </button>
        </div>
      </dialog>
    </div>,
    document.body,
  );
}
