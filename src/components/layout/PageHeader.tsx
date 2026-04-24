import type { ReactNode } from 'react';

interface PageHeaderProps {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}

export function PageHeader({ title, subtitle, actions }: PageHeaderProps) {
  return (
    <div className="page-header">
      <h1 className="page-header-title text-gradient">{title}</h1>
      {subtitle && <p className="page-header-subtitle">{subtitle}</p>}
      {actions && <div className="page-header-actions">{actions}</div>}
    </div>
  );
}
