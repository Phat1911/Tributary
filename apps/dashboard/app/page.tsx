const styles = {
  main: {
    minHeight: "100dvh",
    display: "flex",
    flexDirection: "column" as const,
    alignItems: "center",
    justifyContent: "center",
    gap: "0.75rem",
    padding: "2rem",
    textAlign: "center" as const,
  },
  tagline: {
    opacity: 0.7,
  },
  note: {
    fontSize: "0.875rem",
    opacity: 0.5,
  },
};

export default function Home() {
  return (
    <main style={styles.main}>
      <h1>Tributary</h1>
      <p style={styles.tagline}>Small streams. Pooled power.</p>
      <p style={styles.note}>Dashboard placeholder — the real UI arrives in M3.</p>
    </main>
  );
}
