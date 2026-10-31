## Migration

The fetcher keeps its signature. What changes is what it does when a
request fails: it retries a temporary error up to three times and waits
longer before each try.
