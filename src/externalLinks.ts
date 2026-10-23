export function steamWorkshopUrl(workshopId: string): string {
  return `steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=${encodeURIComponent(workshopId)}`;
}

export function steamWorkshopFallbackUrl(workshopId: string): string {
  return `https://steamcommunity.com/sharedfiles/filedetails/?id=${encodeURIComponent(workshopId)}`;
}
