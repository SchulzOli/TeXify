---
template: invoice
sender:
  name: Max Mustermann IT-Beratung
  street: Musterstraße 1
  zip: "04109"
  city: Leipzig
  phone: +49 170 1234567
  email: rechnung@example.com
  tax_id: 231/123/45678
  bank:
    name: Musterbank Leipzig
    iban: DE12 3456 7890 1234 5678 90
    bic: MUSTDEXXX
recipient: |
  Beispiel GmbH
  Buchhaltung
  Hauptstraße 5
  10115 Berlin
number: 2026-017
customer_number: K-1042
service_period: September 2026
items:
  - desc: Beratung Systemarchitektur
    detail: Workshops vor Ort, 12.–14.09.2026
    qty: 16
    unit: Std.
    price: 110
  - desc: Implementierung **CI/CD-Pipeline**
    qty: 1
    unit: Pausch.
    price: 1850
  - desc: Reisekosten
    qty: 1
    unit: Pausch.
    price: 214.60
---

vielen Dank für Ihren Auftrag. Für die erbrachten Leistungen erlaube ich mir,
wie folgt zu berechnen:
