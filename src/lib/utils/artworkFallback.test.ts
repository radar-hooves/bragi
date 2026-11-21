import { describe, it, expect } from 'vitest';
import { handleArtworkError, resetArtwork } from './artworkFallback';

function fixture() {
  const container = document.createElement('div');
  container.innerHTML = '<img src="https://example.test/first.jpg"><span style="display:none"></span>';
  const img = container.firstElementChild as HTMLImageElement;
  const placeholder = img.nextElementSibling as HTMLElement;
  return { img, placeholder, fail: () => handleArtworkError({ currentTarget: img } as unknown as Event, 'https://example.test/fallback.jpg') };
}

describe('reused artwork', () => {
  it('tries the fallback once, then shows the placeholder without looping', () => {
    const { img, placeholder, fail } = fixture();
    fail();
    expect(img.src).toBe('https://example.test/fallback.jpg');
    fail();
    expect(img.style.display).toBe('none');
    expect(placeholder.style.display).toBe('');
    fail();
    expect(img.dataset.artworkStage).toBe('failed');
  });

  it('shows the next source and restores its fallback after a failed artist', () => {
    const { img, placeholder, fail } = fixture();
    const action = resetArtwork(img, img.src);
    fail(); fail();
    img.src = 'https://example.test/next.jpg';
    action.update(img.src);
    expect(img.style.display).toBe('');
    expect(placeholder.style.display).toBe('none');
    expect(img.dataset.artworkStage).toBeUndefined();
    fail();
    expect(img.src).toBe('https://example.test/fallback.jpg');
  });

  it('does not reset a failing source on an unrelated render or hide a backdrop overlay', () => {
    const { img, placeholder, fail } = fixture();
    placeholder.style.display = '';
    const source = img.src;
    const action = resetArtwork(img, source);
    expect(placeholder.style.display).toBe('');
    fail(); fail();
    action.update(source);
    expect(img.dataset.artworkStage).toBe('failed');
    action.update('next');
    expect(placeholder.style.display).toBe('');
  });
});
