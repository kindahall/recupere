import { describe, expect, it } from 'vitest';
import type { RecoveredFile } from '../types';
import {
  buildFileTree,
  computeVirtualTreeWindow,
  filterFilesForTree,
  flattenVisibleFileTree,
} from './fileTree';

function makeFile(
  overrides: Partial<RecoveredFile> & Pick<RecoveredFile, 'id' | 'name'>,
): RecoveredFile {
  const { id, name, ...rest } = overrides;

  return {
    id,
    name,
    path: rest.path ?? '/',
    extension: rest.extension ?? 'txt',
    sizeBytes: rest.sizeBytes ?? 10,
    integrity: rest.integrity ?? 'intact',
    recoveryScore: rest.recoveryScore ?? 95,
    recoveryMethod: rest.recoveryMethod ?? 'filesystem',
    previewAvailable: rest.previewAvailable ?? true,
    ...rest,
  };
}

describe('buildFileTree', () => {
  it('creates nested directories from display paths', () => {
    const tree = buildFileTree([
      makeFile({ id: 'file-1', name: 'report.txt', path: '/cases/client-a' }),
      makeFile({
        id: 'file-2',
        name: 'photo.jpg',
        path: '/cases/client-a/media',
        extension: 'jpg',
      }),
      makeFile({ id: 'file-3', name: 'notes.txt', path: '/' }),
    ]);

    expect(tree.children?.map((node) => node.name)).toEqual(['cases', 'notes.txt']);
    expect(tree.children?.[0].children?.map((node) => node.name)).toEqual(['client-a']);
    expect(tree.children?.[0].children?.[0].children?.map((node) => node.name)).toEqual([
      'media',
      'report.txt',
    ]);
    expect(
      tree.children?.[0].children?.[0].children?.[0].children?.map((node) => node.name),
    ).toEqual(['photo.jpg']);
  });

  it('sorts directories before files with a stable alphabetical order', () => {
    const tree = buildFileTree([
      makeFile({ id: 'file-1', name: 'zeta.txt', path: '/' }),
      makeFile({ id: 'file-2', name: 'alpha.txt', path: '/' }),
      makeFile({ id: 'file-3', name: 'inside.txt', path: '/Beta' }),
      makeFile({ id: 'file-4', name: 'inside-2.txt', path: '/alpha' }),
    ]);

    expect(tree.children?.map((node) => node.name)).toEqual([
      'alpha',
      'Beta',
      'alpha.txt',
      'zeta.txt',
    ]);
  });

  it('keeps file payloads attached to leaf nodes', () => {
    const tree = buildFileTree([
      makeFile({
        id: 'file-1',
        name: 'archive.bin',
        path: '/restored',
        recoveryMethod: 'reconstruction',
        isDeleted: true,
      }),
    ]);

    const leaf = tree.children?.[0].children?.[0];
    expect(leaf?.isDirectory).toBe(false);
    expect(leaf?.file?.id).toBe('file-1');
    expect(leaf?.file?.recoveryMethod).toBe('reconstruction');
    expect(leaf?.file?.isDeleted).toBe(true);
  });

  it('filters tree files by text query and mode', () => {
    const files = [
      makeFile({ id: 'file-1', name: 'report.txt', path: '/cases/client-a', isDeleted: true }),
      makeFile({
        id: 'file-2',
        name: 'photo.jpg',
        path: '/cases/client-a/media',
        extension: 'jpg',
        previewAvailable: true,
      }),
      makeFile({
        id: 'file-3',
        name: 'raw.bin',
        path: '/dump',
        extension: 'bin',
        previewAvailable: false,
      }),
    ];

    expect(
      filterFilesForTree(files, {
        query: 'client-a',
      }).map((file) => file.id),
    ).toEqual(['file-1', 'file-2']);

    expect(
      filterFilesForTree(files, {
        mode: 'deleted',
      }).map((file) => file.id),
    ).toEqual(['file-1']);

    expect(
      filterFilesForTree(files, {
        mode: 'previewable',
      }).map((file) => file.id),
    ).toEqual(['file-1', 'file-2']);

    expect(
      filterFilesForTree(files, {
        mode: 'selected',
        selectedFileIds: new Set(['file-2']),
      }).map((file) => file.id),
    ).toEqual(['file-2']);
  });

  it('flattens only expanded tree branches in visible order', () => {
    const tree = buildFileTree([
      makeFile({ id: 'file-1', name: 'report.txt', path: '/cases/client-a' }),
      makeFile({
        id: 'file-2',
        name: 'photo.jpg',
        path: '/cases/client-a/media',
        extension: 'jpg',
      }),
      makeFile({ id: 'file-3', name: 'notes.txt', path: '/' }),
    ]);

    expect(
      flattenVisibleFileTree(tree, new Set(['root/cases', 'root/cases/client-a'])).map(
        (row) => row.node.name,
      ),
    ).toEqual(['cases', 'client-a', 'media', 'report.txt', 'notes.txt']);
  });

  it('computes a stable virtual window with overscan', () => {
    expect(computeVirtualTreeWindow(100, 240, 180, 36, 4)).toEqual({
      startIndex: 2,
      endIndex: 15,
      offsetTop: 72,
      totalHeight: 3600,
    });
  });

  it('bounds the visible window on large trees so only a slice is mounted', () => {
    const ROW_HEIGHT = 36;
    const VIEWPORT = 420;
    const OVERSCAN = 6;
    const TOTAL = 10_000;

    const top = computeVirtualTreeWindow(TOTAL, 0, VIEWPORT, ROW_HEIGHT, OVERSCAN);
    const middle = computeVirtualTreeWindow(
      TOTAL,
      5_000 * ROW_HEIGHT,
      VIEWPORT,
      ROW_HEIGHT,
      OVERSCAN,
    );
    const bottom = computeVirtualTreeWindow(
      TOTAL,
      (TOTAL - 2) * ROW_HEIGHT,
      VIEWPORT,
      ROW_HEIGHT,
      OVERSCAN,
    );

    const visible = (window: ReturnType<typeof computeVirtualTreeWindow>) =>
      window.endIndex - window.startIndex;

    const upperBound = Math.ceil(VIEWPORT / ROW_HEIGHT) + OVERSCAN * 2 + 1;
    expect(visible(top)).toBeLessThanOrEqual(upperBound);
    expect(visible(middle)).toBeLessThanOrEqual(upperBound);
    expect(visible(bottom)).toBeLessThanOrEqual(upperBound);

    expect(top.startIndex).toBe(0);
    expect(middle.startIndex).toBe(5_000 - OVERSCAN);
    expect(middle.endIndex).toBeLessThan(TOTAL);
    expect(bottom.endIndex).toBe(TOTAL);
    expect(top.totalHeight).toBe(TOTAL * ROW_HEIGHT);
  });
});
