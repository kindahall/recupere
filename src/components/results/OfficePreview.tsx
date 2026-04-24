import { FileSpreadsheet, FileText, Presentation } from 'lucide-react';
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';

interface OfficePreviewProps {
  extension: string;
  textContent: string;
  expert?: boolean;
}

const SPEAKER_NOTES_MARKER = '[Speaker Notes]';

export function OfficePreview({ extension, textContent, expert = false }: OfficePreviewProps) {
  const { t } = useTranslation();
  const normalizedExtension = extension.toLowerCase();

  if (normalizedExtension === 'xlsx') {
    return <XlsxPreview textContent={textContent} expert={expert} t={t} />;
  }

  if (normalizedExtension === 'pptx') {
    return <PptxPreview textContent={textContent} expert={expert} t={t} />;
  }

  if (normalizedExtension === 'docx') {
    return <DocxPreview textContent={textContent} expert={expert} t={t} />;
  }

  return (
    <pre
      style={{
        margin: 0,
        maxHeight: 360,
        overflow: 'auto',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        padding: 'var(--space-4)',
        borderRadius: 'var(--radius-lg)',
        background: 'var(--color-bg-secondary)',
        border: '1px solid var(--color-border)',
        fontFamily: 'var(--font-mono)',
        fontSize: 'var(--font-size-sm)',
      }}
    >
      {textContent}
    </pre>
  );
}

interface SubPreviewProps {
  textContent: string;
  expert: boolean;
  t: (key: string, options?: Record<string, unknown>) => string;
}

function DocxPreview({ textContent, expert, t }: SubPreviewProps) {
  const paragraphs = useMemo(() => splitParagraphs(textContent), [textContent]);

  return (
    <div
      className="office-preview office-preview--docx"
      style={{
        maxHeight: 420,
        overflow: 'auto',
        padding: 'var(--space-5)',
        borderRadius: 'var(--radius-lg)',
        background: 'var(--color-bg)',
        border: '1px solid var(--color-border)',
      }}
    >
      <div className="flex items-center gap-2" style={{ marginBottom: 'var(--space-3)' }}>
        <FileText size={16} className="text-accent" />
        <span className="font-medium">{t('results.office.docx_label')}</span>
        {expert && (
          <span className="text-xs text-secondary" style={{ marginInlineStart: 'auto' }}>
            {t('results.office.docx_expert_meta', {
              paragraphs: paragraphs.length,
              characters: textContent.length,
            })}
          </span>
        )}
      </div>
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: 'var(--space-3)',
          fontFamily: 'var(--font-body, var(--font-sans))',
          fontSize: 'var(--font-size-md)',
          lineHeight: 1.55,
          color: 'var(--color-text)',
        }}
      >
        {paragraphs.length === 0 ? (
          <p className="text-sm text-secondary">{t('results.office.docx_empty')}</p>
        ) : (
          paragraphs.map((paragraph, index) => (
            <p
              key={`p-${index}-${paragraph.slice(0, 16)}`}
              style={{ margin: 0, whiteSpace: 'pre-wrap' }}
            >
              {paragraph}
            </p>
          ))
        )}
      </div>
    </div>
  );
}

function XlsxPreview({ textContent, expert, t }: SubPreviewProps) {
  const sheets = useMemo(() => parseXlsxSheets(textContent), [textContent]);
  const totalCells = useMemo(
    () =>
      sheets.reduce((sum, sheet) => sum + sheet.rows.reduce((acc, row) => acc + row.length, 0), 0),
    [sheets],
  );

  return (
    <div
      className="office-preview office-preview--xlsx"
      style={{
        maxHeight: 480,
        overflow: 'auto',
        padding: 'var(--space-4)',
        borderRadius: 'var(--radius-lg)',
        background: 'var(--color-bg)',
        border: '1px solid var(--color-border)',
      }}
    >
      <div className="flex items-center gap-2" style={{ marginBottom: 'var(--space-3)' }}>
        <FileSpreadsheet size={16} className="text-accent" />
        <span className="font-medium">{t('results.office.xlsx_label')}</span>
        {expert && (
          <span className="text-xs text-secondary" style={{ marginInlineStart: 'auto' }}>
            {t('results.office.xlsx_expert_meta', {
              sheets: sheets.length,
              cells: totalCells,
            })}
          </span>
        )}
      </div>
      {sheets.length === 0 ? (
        <p className="text-sm text-secondary">{t('results.office.xlsx_empty')}</p>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-4)' }}>
          {sheets.map((sheet, sheetIndex) => (
            <section key={`sheet-${sheetIndex}`} aria-label={sheet.label ?? undefined}>
              <header className="text-xs text-secondary" style={{ marginBottom: 'var(--space-1)' }}>
                {sheet.label ?? t('results.office.xlsx_sheet_default', { index: sheetIndex + 1 })}
              </header>
              <div style={{ overflowX: 'auto' }}>
                <table
                  className="office-preview__table"
                  style={{
                    width: '100%',
                    borderCollapse: 'collapse',
                    fontFamily: 'var(--font-mono)',
                    fontSize: 'var(--font-size-sm)',
                  }}
                >
                  <tbody>
                    {sheet.rows.map((row, rowIndex) => (
                      <tr key={`row-${sheetIndex}-${rowIndex}`}>
                        {row.map((cell, cellIndex) => (
                          <td
                            key={`cell-${sheetIndex}-${rowIndex}-${cellIndex}`}
                            style={{
                              border: '1px solid var(--color-border)',
                              padding: 'var(--space-1) var(--space-2)',
                              background:
                                rowIndex === 0 ? 'var(--color-bg-secondary)' : 'transparent',
                              fontWeight: rowIndex === 0 ? 600 : 400,
                              whiteSpace: 'pre-wrap',
                              wordBreak: 'break-word',
                              verticalAlign: 'top',
                            }}
                          >
                            {cell}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

function PptxPreview({ textContent, expert, t }: SubPreviewProps) {
  const slides = useMemo(() => parsePptxSlides(textContent), [textContent]);
  const notesCount = useMemo(
    () => slides.reduce((count, slide) => count + (slide.notes ? 1 : 0), 0),
    [slides],
  );

  return (
    <div
      className="office-preview office-preview--pptx"
      style={{
        maxHeight: 520,
        overflow: 'auto',
        padding: 'var(--space-4)',
        borderRadius: 'var(--radius-lg)',
        background: 'var(--color-bg)',
        border: '1px solid var(--color-border)',
      }}
    >
      <div className="flex items-center gap-2" style={{ marginBottom: 'var(--space-3)' }}>
        <Presentation size={16} className="text-accent" />
        <span className="font-medium">{t('results.office.pptx_label')}</span>
        {expert && (
          <span className="text-xs text-secondary" style={{ marginInlineStart: 'auto' }}>
            {t('results.office.pptx_expert_meta', {
              slides: slides.length,
              notes: notesCount,
            })}
          </span>
        )}
      </div>
      {slides.length === 0 ? (
        <p className="text-sm text-secondary">{t('results.office.pptx_empty')}</p>
      ) : (
        <ol
          style={{
            listStyle: 'none',
            padding: 0,
            margin: 0,
            display: 'flex',
            flexDirection: 'column',
            gap: 'var(--space-4)',
          }}
        >
          {slides.map((slide, index) => (
            <li
              key={`slide-${index}`}
              style={{
                padding: 'var(--space-4)',
                borderRadius: 'var(--radius-md)',
                background: 'var(--color-bg-secondary)',
                border: '1px solid var(--color-border)',
              }}
            >
              <header className="text-xs text-secondary" style={{ marginBottom: 'var(--space-2)' }}>
                {t('results.office.pptx_slide_heading', { index: index + 1 })}
              </header>
              <div
                style={{
                  whiteSpace: 'pre-wrap',
                  fontFamily: 'var(--font-body, var(--font-sans))',
                  fontSize: 'var(--font-size-md)',
                  lineHeight: 1.5,
                  color: 'var(--color-text)',
                }}
              >
                {slide.body || (
                  <span className="text-sm text-secondary">
                    {t('results.office.pptx_slide_empty')}
                  </span>
                )}
              </div>
              {slide.notes && (
                <aside
                  style={{
                    marginTop: 'var(--space-3)',
                    paddingTop: 'var(--space-2)',
                    borderTop: '1px dashed var(--color-border)',
                    fontSize: 'var(--font-size-sm)',
                    color: 'var(--color-text-secondary)',
                    whiteSpace: 'pre-wrap',
                  }}
                >
                  <span
                    className="font-medium"
                    style={{ display: 'block', marginBottom: 'var(--space-1)' }}
                  >
                    {t('results.office.pptx_speaker_notes')}
                  </span>
                  {slide.notes}
                </aside>
              )}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

export function splitParagraphs(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

export interface XlsxSheet {
  label: string | null;
  rows: string[][];
}

export function parseXlsxSheets(text: string): XlsxSheet[] {
  if (!text.trim()) {
    return [];
  }

  return text
    .split(/\r?\n\r?\n/)
    .map((section) => section.trim())
    .filter((section) => section.length > 0)
    .map((section) => {
      const lines = section.split(/\r?\n/);
      let label: string | null = null;
      let startIndex = 0;

      const firstLine = lines[0]?.trim() ?? '';
      if (firstLine && !firstLine.includes('\t')) {
        label = firstLine;
        startIndex = 1;
      }

      const rows = lines
        .slice(startIndex)
        .map((line) => line.split('\t'))
        .filter((row) => row.some((cell) => cell.trim().length > 0));

      return { label, rows };
    })
    .filter((sheet) => sheet.rows.length > 0 || sheet.label !== null);
}

export interface PptxSlide {
  body: string;
  notes: string | null;
}

export function parsePptxSlides(text: string): PptxSlide[] {
  if (!text.trim()) {
    return [];
  }

  const marker = SPEAKER_NOTES_MARKER;
  const tokens = text.split(/\r?\n\r?\n/);
  const slides: PptxSlide[] = [];
  let current: string[] = [];

  const flush = () => {
    if (current.length === 0) {
      return;
    }
    const combined = current.join('\n\n').trim();
    if (!combined) {
      current = [];
      return;
    }
    const notesIndex = combined.indexOf(marker);
    if (notesIndex === -1) {
      slides.push({ body: combined, notes: null });
    } else {
      const body = combined.slice(0, notesIndex).trim();
      const notes = combined
        .slice(notesIndex + marker.length)
        .replace(/^\r?\n/, '')
        .trim();
      slides.push({ body, notes: notes.length > 0 ? notes : null });
    }
    current = [];
  };

  for (const token of tokens) {
    const trimmed = token.trim();
    if (trimmed.startsWith(marker)) {
      current.push(token);
      flush();
      continue;
    }
    if (current.length > 0 && !current[current.length - 1].includes(marker)) {
      flush();
    }
    current.push(token);
  }
  flush();

  return slides;
}
