interface ProgressTimelineProps {
  percent: number;
  label?: string;
  sublabel?: string;
  animated?: boolean;
}

export function ProgressTimeline({
  percent,
  label,
  sublabel,
  animated = false,
}: ProgressTimelineProps) {
  return (
    <div className="progress-timeline">
      <div className="progress-bar-container">
        <div
          className={`progress-bar-fill${animated ? ' animated' : ''}`}
          style={{ width: `${Math.min(100, Math.max(0, percent))}%` }}
        />
      </div>
      <div className="progress-stats">
        <span>{label || `${percent.toFixed(1)}%`}</span>
        {sublabel && <span>{sublabel}</span>}
      </div>
    </div>
  );
}
