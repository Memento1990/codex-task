export const utilityKinds = ['settings', 'integration', 'updates', 'notifications', 'repair', 'weixin-lab'] as const;
export type UtilityKind = typeof utilityKinds[number];
export function utilityKind(value: unknown): UtilityKind | null {
  return typeof value === 'string' && (utilityKinds as readonly string[]).includes(value) ? value as UtilityKind : null;
}
