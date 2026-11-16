// Single source of truth for the promo: scene order, durations (multiples of
// the 0.6 s beat so every cut lands on a kick), chords and voice-over lines.
// Read by story.html (as a script) and by audio.mjs / voice.mjs (parsed as JSON).
// "Ai clir" is how the Spanish voice gets to pronounce "iclear".
window.TIMELINE = {
  "beat": 0.6,
  "scenes": [
    { "id": "hook",     "dur": 3.6, "chord": [57, 60, 64, 67, 71], "bass": 33, "vo": "Tu Mac no está viejo. Está saturado." },
    { "id": "intro",    "dur": 4.8, "chord": [53, 57, 60, 64, 67], "bass": 29, "vo": "Te presento Ai clir: el centro de control que tu Mac necesitaba." },
    { "id": "memory",   "dur": 4.8, "chord": [55, 60, 64, 67, 71], "bass": 36, "vo": "Detecta en segundos qué está devorando tu memoria, y por qué." },
    { "id": "recs",     "dur": 4.8, "chord": [55, 59, 62, 64, 67], "bass": 31, "vo": "Recomendaciones inteligentes: recupera gigas con un solo toque." },
    { "id": "apps",     "dur": 4.8, "chord": [57, 60, 64, 67, 71], "bass": 33, "vo": "Elimina las apps que ya no usas, sin dejar ni un archivo atrás." },
    { "id": "cleanup",  "dur": 4.2, "chord": [53, 57, 60, 64, 67], "bass": 29, "vo": "Limpia cachés y basura, siempre con vista previa." },
    { "id": "control",  "dur": 4.2, "chord": [55, 60, 64, 67, 71], "bass": 36, "vo": "Batería, ventiladores y temperatura, bajo control." },
    { "id": "optimize", "dur": 4.2, "chord": [55, 59, 62, 64, 67], "bass": 31, "vo": "Optimiza con un clic, desde tu barra de menú." },
    { "id": "trust",    "dur": 4.2, "chord": [57, 60, 64, 67, 72], "bass": 33, "vo": "Todo es local: tus datos nunca salen de tu Mac." },
    { "id": "end",      "dur": 4.8, "chord": [60, 64, 67, 71, 74, 79], "bass": 36, "vo": "Ai clir. Tu Mac, como nuevo. Síguenos para el lanzamiento." }
  ]
};
