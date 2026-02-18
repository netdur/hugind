// Simple local dialog helper used across the app.
// In a real app this might bridge to a modal provider.
export function showErrorDialog({ title, message }) {
  // eslint-disable-next-line no-alert
  alert(`${title}\n\n${message}`);
}
