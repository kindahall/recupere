import { describe, expect, it } from 'vitest';
import { parsePptxSlides, parseXlsxSheets, splitParagraphs } from './OfficePreview';

describe('splitParagraphs', () => {
  it('returns trimmed non-empty lines', () => {
    const paragraphs = splitParagraphs('Intro\n\nBody paragraph.\n   \nClosing.');
    expect(paragraphs).toEqual(['Intro', 'Body paragraph.', 'Closing.']);
  });

  it('handles Windows line endings', () => {
    expect(splitParagraphs('First\r\nSecond\r\n\r\nThird')).toEqual(['First', 'Second', 'Third']);
  });

  it('returns an empty array for empty input', () => {
    expect(splitParagraphs('')).toEqual([]);
    expect(splitParagraphs('   \n  \n')).toEqual([]);
  });
});

describe('parseXlsxSheets', () => {
  it('parses a single sheet with header and data rows', () => {
    const text = 'Name\tAge\nAlice\t32\nBob\t27';
    const sheets = parseXlsxSheets(text);
    expect(sheets).toHaveLength(1);
    expect(sheets[0].label).toBeNull();
    expect(sheets[0].rows).toEqual([
      ['Name', 'Age'],
      ['Alice', '32'],
      ['Bob', '27'],
    ]);
  });

  it('extracts sheet label when first line has no tab', () => {
    const text = 'Summary\nName\tAge\nAlice\t32\n\nDetails\nCity\nParis';
    const sheets = parseXlsxSheets(text);
    expect(sheets).toHaveLength(2);
    expect(sheets[0].label).toBe('Summary');
    expect(sheets[0].rows).toEqual([
      ['Name', 'Age'],
      ['Alice', '32'],
    ]);
    expect(sheets[1].label).toBe('Details');
    expect(sheets[1].rows).toEqual([['City'], ['Paris']]);
  });

  it('returns an empty array for blank input', () => {
    expect(parseXlsxSheets('')).toEqual([]);
    expect(parseXlsxSheets('   \n  ')).toEqual([]);
  });

  it('drops rows with only whitespace cells', () => {
    const text = 'A\tB\n  \t  \nC\tD';
    const sheets = parseXlsxSheets(text);
    expect(sheets).toHaveLength(1);
    expect(sheets[0].rows).toEqual([
      ['A', 'B'],
      ['C', 'D'],
    ]);
  });
});

describe('parsePptxSlides', () => {
  it('splits slides separated by blank lines without notes', () => {
    const text = 'Title slide\nAgenda\n\nChapter 1\nOverview\n\nChapter 2\nDetails';
    const slides = parsePptxSlides(text);
    expect(slides).toHaveLength(3);
    expect(slides[0]).toEqual({ body: 'Title slide\nAgenda', notes: null });
    expect(slides[1]).toEqual({ body: 'Chapter 1\nOverview', notes: null });
    expect(slides[2]).toEqual({ body: 'Chapter 2\nDetails', notes: null });
  });

  it('extracts speaker notes attached to a slide', () => {
    const text =
      'Slide body line 1\nSlide body line 2\n\n[Speaker Notes]\nRemember to introduce the release metrics';
    const slides = parsePptxSlides(text);
    expect(slides).toHaveLength(1);
    expect(slides[0]).toEqual({
      body: 'Slide body line 1\nSlide body line 2',
      notes: 'Remember to introduce the release metrics',
    });
  });

  it('handles mixed slides with and without notes', () => {
    const text = [
      'Cover',
      'Welcome to Q3 review',
      '',
      '[Speaker Notes]',
      'Open with the exec summary',
      '',
      'Metrics',
      'Revenue +12%',
      '',
      'Closing',
    ].join('\n');

    const slides = parsePptxSlides(text);
    expect(slides).toHaveLength(3);
    expect(slides[0]).toEqual({
      body: 'Cover\nWelcome to Q3 review',
      notes: 'Open with the exec summary',
    });
    expect(slides[1]).toEqual({ body: 'Metrics\nRevenue +12%', notes: null });
    expect(slides[2]).toEqual({ body: 'Closing', notes: null });
  });

  it('returns empty array for blank input', () => {
    expect(parsePptxSlides('')).toEqual([]);
    expect(parsePptxSlides('\n   \n')).toEqual([]);
  });
});
