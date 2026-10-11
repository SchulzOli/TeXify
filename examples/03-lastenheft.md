---
template: article
subtitle: Online-Terminbuchung für die Praxis Dr. Beispiel
author: Max Mustermann
date: 2026-09-30
numbered: true
toc: true
font: serif
accent: 6A1B9A
---

# Lastenheft

## Ausgangssituation

Termine werden heute ausschließlich telefonisch vergeben. In Stoßzeiten ist
die Praxis kaum erreichbar, rund 15 % der Termine werden nicht wahrgenommen.

## Zielsetzung

Patientinnen und Patienten sollen Termine rund um die Uhr online buchen,
verschieben und absagen können. Die Ausfallquote soll unter 5 % sinken.

## Funktionale Anforderungen

| ID   | Anforderung                                         | Priorität |
|:-----|:----------------------------------------------------|:---------:|
| F-01 | Freie Termine nach Behandlungsart anzeigen          | Muss      |
| F-02 | Termin buchen mit Bestätigung per E-Mail            | Muss      |
| F-03 | Erinnerung 24 Stunden vor dem Termin                | Muss      |
| F-04 | Termin online verschieben oder absagen              | Soll      |
| F-05 | Warteliste für kurzfristig frei werdende Termine    | Kann      |

## Nicht-funktionale Anforderungen

- **Datenschutz:** Verarbeitung ausschließlich auf Servern in der EU, DSGVO-konform
- **Verfügbarkeit:** mindestens 99,5 % im Monatsmittel
- **Barrierefreiheit:** Konformität mit WCAG 2.2, Stufe AA

## Abnahmekriterien

1. Alle Muss-Anforderungen sind umgesetzt und getestet.
2. Ein Pilotbetrieb über vier Wochen verläuft ohne kritische Fehler.
