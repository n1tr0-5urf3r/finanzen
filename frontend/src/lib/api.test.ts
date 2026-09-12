import { afterEach, describe, expect, it, vi } from 'vitest';

import { apiBlob, downloadFile } from './api';

function respondWith(disposition: string) {
  const fetchMock = vi.fn().mockResolvedValue(
    new Response(new Blob(['x']), {
      status: 200,
      headers: { 'Content-Disposition': disposition },
    }),
  );
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('apiBlob', () => {
  /**
   * The umlaut path everybody breaks. `Belegübersicht_2026.pdf` travels
   * percent-encoded in `filename*`; a client that reads only the ASCII `filename=`
   * saves it under the mangled fallback, and one that reads `filename*` without
   * decoding saves it as `Beleg%C3%BCbersicht_2026.pdf`.
   */
  it('prefers the RFC 5987 filename and percent-decodes it', async () => {
    respondWith(
      "attachment; filename=\"Belegubersicht_2026.pdf\"; filename*=UTF-8''Beleg%C3%BCbersicht_2026.pdf",
    );
    const { filename } = await apiBlob('/tax/export.pdf?year=2026');
    expect(filename).toBe('Belegübersicht_2026.pdf');
  });

  it('falls back to the plain filename when there is no encoded one', async () => {
    respondWith('attachment; filename="Steuer_2026.csv"');
    expect((await apiBlob('/tax/export.csv?year=2026')).filename).toBe('Steuer_2026.csv');
  });

  it('never invents a name when the header is absent', async () => {
    respondWith('attachment');
    expect((await apiBlob('/x')).filename).toBe('download');
  });

  it('raises rather than saving an error page as a file', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response('{"message":"nope"}', { status: 404 })),
    );
    await expect(apiBlob('/tax/export.csv?year=1900')).rejects.toThrow();
  });
});

describe('downloadFile', () => {
  it('hands the blob to the browser under the decoded name', async () => {
    respondWith("attachment; filename*=UTF-8''Beleg%C3%BCbersicht_2026.pdf");
    // jsdom has neither. They are assigned rather than stubbed and are left in
    // place afterwards: the revoke runs on a later tick, and restoring the bare
    // jsdom URL before then would throw outside any test.
    const objectUrl = URL as unknown as {
      createObjectURL: (blob: Blob) => string;
      revokeObjectURL: (url: string) => void;
    };
    objectUrl.createObjectURL = vi.fn().mockReturnValue('blob:fake');
    objectUrl.revokeObjectURL = vi.fn();
    const click = vi
      .spyOn(HTMLAnchorElement.prototype, 'click')
      .mockImplementation(() => undefined);

    const filename = await downloadFile('/tax/export.pdf?year=2026');

    expect(filename).toBe('Belegübersicht_2026.pdf');
    expect(click).toHaveBeenCalledOnce();
    // The anchor is removed again: a download must not leave DOM behind.
    expect(document.querySelectorAll('a[download]')).toHaveLength(0);
  });
});
