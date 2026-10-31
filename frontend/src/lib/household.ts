/**
 * The analysis page's "Haushaltsanteile einbeziehen" switch.
 *
 * Every statistic counts my share of what the flatmates paid for the household,
 * in the finance category its KitchenOwl category is mapped to, and nets the
 * settlements accordingly. The analysis page can switch that off to see the
 * bookings alone; the backend takes it as `household=false`.
 *
 * The switch lives in the URL like the year and the tab, so a view can be linked
 * and survives a reload. On is the default and is not written: only `haushalt=0`
 * appears, so every link that predates the switch means what it always meant to
 * the backend, which is "the default".
 */
export const HOUSEHOLD_PARAM = 'haushalt';

/** Reads the switch from the page's search params. */
export function householdFromParams(params: URLSearchParams): boolean {
  return params.get(HOUSEHOLD_PARAM) !== '0';
}

/**
 * `path` with the switch appended. Always explicit, so a request says which
 * ledger it asked for rather than relying on the server's default.
 */
export function withHousehold(path: string, household: boolean): string {
  return `${path}${path.includes('?') ? '&' : '?'}household=${household}`;
}
