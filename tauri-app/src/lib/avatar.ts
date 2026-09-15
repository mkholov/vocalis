// Mirrors app.rs's `avatar_color`/`initials` (teacher grid, seat_card) so a
// given name gets the same avatar hue on both stacks — not load-bearing
// anywhere, just a nice continuity touch while both UIs coexist.
export function avatarColor(name: string): string {
  let hash = 2166136261;
  for (let i = 0; i < name.length; i++) {
    hash = Math.imul(hash ^ name.charCodeAt(i), 16777619) >>> 0;
  }
  const hue = hash % 360;
  return `hsl(${hue} 45% 55%)`;
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0][0]!.toUpperCase();
  return (parts[0][0]! + parts[1][0]!).toUpperCase();
}
