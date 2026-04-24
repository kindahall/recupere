import type { ReactNode } from 'react';

interface SectionCardProps {
  title?: string;
  subtitle?: string;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function SectionCard({ title, subtitle, action, children, className }: SectionCardProps) {
  return (
    <div className={`section-card${className ? ` ${className}` : ''}`}>
      {(title || action) && (
        <div className="section-card-header">
          <div>
            {title && <h3 className="section-card-title">{title}</h3>}
            {subtitle && <p className="section-card-subtitle">{subtitle}</p>}
          </div>
          {action}
        </div>
      )}
      {children}
    </div>
  );
}
