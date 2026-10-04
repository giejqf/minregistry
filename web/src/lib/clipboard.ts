/** Copies text, falling back to execCommand where the async clipboard API is unavailable. */
export async function copyText(value: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
    return;
  }
  // Fallback for non-secure contexts (plain-HTTP registries), where the async clipboard API is missing.
  const area = document.createElement('textarea');
  area.value = value;
  area.setAttribute('readonly', '');
  area.style.position = 'fixed';
  area.style.opacity = '0';
  document.body.appendChild(area);
  area.select();
  try {
    // eslint-disable-next-line @typescript-eslint/no-deprecated -- the only option without navigator.clipboard (non-secure contexts)
    if (!document.execCommand('copy')) throw new Error('copy failed');
  } finally {
    document.body.removeChild(area);
  }
}
