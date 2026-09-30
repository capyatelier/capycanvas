export async function runJourney(journeys) {
  const journey = journeys.find(([matches]) => matches);
  if (!journey) return false;
  await journey[1]();
  journey[2]?.();
  return true;
}
