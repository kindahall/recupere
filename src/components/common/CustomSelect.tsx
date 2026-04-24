import { Check, ChevronDown } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

export interface SelectOption {
  value: string;
  label: string;
}

export interface CustomSelectProps {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  icon?: React.ReactNode;
  style?: React.CSSProperties;
  className?: string;
  placeholder?: string;
}

export function CustomSelect({
  id,
  value,
  onChange,
  options,
  icon,
  style,
  className,
  placeholder,
}: CustomSelectProps) {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const resolvedPlaceholder = placeholder ?? t('common.select');

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const selectedOption = options.find((o) => o.value === value);

  return (
    <div
      ref={containerRef}
      style={{ position: 'relative', width: '100%', ...style }}
      className={className}
    >
      <button
        id={id}
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        style={{
          width: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: 'var(--space-2) var(--space-3)',
          borderRadius: 'var(--radius-md)',
          border: `1px solid ${isOpen ? 'var(--color-accent)' : 'rgba(255, 255, 255, 0.1)'}`,
          background: isOpen ? 'rgba(0, 0, 0, 0.5)' : 'rgba(0, 0, 0, 0.2)',
          color: 'var(--color-text-primary)',
          fontSize: 'var(--font-size-sm)',
          outline: 'none',
          transition: 'all var(--transition-fast)',
          boxShadow: isOpen
            ? '0 0 0 2px var(--color-accent-subtle), inset 0 2px 4px rgba(0,0,0,0.2)'
            : 'inset 0 1px 2px rgba(0,0,0,0.1)',
          cursor: 'pointer',
        }}
        onMouseOver={(e) => {
          if (!isOpen) {
            e.currentTarget.style.borderColor = 'var(--color-border-focus)';
            e.currentTarget.style.background = 'rgba(0, 0, 0, 0.3)';
          }
        }}
        onFocus={(e) => {
          if (!isOpen) {
            e.currentTarget.style.borderColor = 'var(--color-border-focus)';
            e.currentTarget.style.background = 'rgba(0, 0, 0, 0.3)';
          }
        }}
        onMouseOut={(e) => {
          if (!isOpen) {
            e.currentTarget.style.borderColor = 'rgba(255, 255, 255, 0.1)';
            e.currentTarget.style.background = 'rgba(0, 0, 0, 0.2)';
          }
        }}
        onBlur={(e) => {
          if (!isOpen) {
            e.currentTarget.style.borderColor = 'rgba(255, 255, 255, 0.1)';
            e.currentTarget.style.background = 'rgba(0, 0, 0, 0.2)';
          }
        }}
      >
        <span
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 'var(--space-2)',
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
          }}
        >
          {icon && (
            <span
              style={{ color: 'var(--color-text-muted)', display: 'flex', alignItems: 'center' }}
            >
              {icon}
            </span>
          )}
          {selectedOption ? selectedOption.label : resolvedPlaceholder}
        </span>
        <ChevronDown
          size={14}
          style={{
            color: 'var(--color-text-muted)',
            transform: isOpen ? 'rotate(180deg)' : 'none',
            transition: 'transform 0.3s cubic-bezier(0.4, 0, 0.2, 1)',
            flexShrink: 0,
          }}
        />
      </button>

      {isOpen && (
        <div
          style={{
            position: 'absolute',
            top: '100%',
            left: 0,
            right: 0,
            marginTop: '8px',
            background: 'var(--color-bg-primary)',
            border: '1px solid var(--color-border)',
            borderRadius: 'var(--radius-md)',
            boxShadow: 'var(--shadow-xl)',
            zIndex: 1000,
            padding: 'var(--space-1)',
            maxHeight: '280px',
            overflowY: 'auto',
          }}
        >
          {options.length === 0 ? (
            <div
              style={{
                padding: 'var(--space-2) var(--space-3)',
                color: 'var(--color-text-muted)',
                fontSize: 'var(--font-size-sm)',
              }}
            >
              {t('common.no_options')}
            </div>
          ) : (
            options.map((o) => {
              const isSelected = value === o.value;
              return (
                <button
                  key={o.value}
                  type="button"
                  onClick={() => {
                    onChange(o.value);
                    setIsOpen(false);
                  }}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 'var(--space-2)',
                    width: '100%',
                    padding: '8px 12px',
                    borderRadius: 'var(--radius-sm)',
                    background: isSelected ? 'var(--color-accent-subtle)' : 'transparent',
                    color: isSelected ? 'var(--color-accent)' : 'var(--color-text-primary)',
                    fontSize: 'var(--font-size-sm)',
                    textAlign: 'left',
                    cursor: 'pointer',
                    border: 'none',
                    transition: 'background 0.2s, color 0.2s',
                  }}
                  onMouseOver={(e) => {
                    if (!isSelected) {
                      e.currentTarget.style.background = 'var(--color-bg-sidebar-hover)';
                    }
                  }}
                  onFocus={(e) => {
                    if (!isSelected) {
                      e.currentTarget.style.background = 'var(--color-bg-sidebar-hover)';
                    }
                  }}
                  onMouseOut={(e) => {
                    if (!isSelected) {
                      e.currentTarget.style.background = 'transparent';
                    }
                  }}
                  onBlur={(e) => {
                    if (!isSelected) {
                      e.currentTarget.style.background = 'transparent';
                    }
                  }}
                >
                  <span
                    style={{
                      width: '14px',
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'center',
                    }}
                  >
                    {isSelected && <Check size={14} />}
                  </span>
                  <span style={{ flex: 1, lineHeight: '1.4', wordBreak: 'break-word' }}>
                    {o.label}
                  </span>
                </button>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
