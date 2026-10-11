---
template: invoice
sender:
  name: Robin Lee Photography
  street: 4 Linden Way
  zip: "04229"
  city: Leipzig
  email: hello@example.net
  phone: ""            # clear values inherited from texify.toml
  web: ""
  tax_id: 231/987/65432
  bank:
    name: Sample Bank
    iban: DE98 7654 3210 9876 5432 10
    bic: SMPLDEXXX
recipient: |
  The Taylor Family
  3 Park Road
  04155 Leipzig
number: RL-2026-112
service_date: 2026-10-03
small_business: true
due_days: 10
accent: 00796B
opening: Dear Taylor family,
items:
  - {desc: Outdoor family photo shoot, qty: 2, unit: h, price: 120}
  - {desc: Image editing, detail: 25 photos incl. retouching, qty: 25, unit: photo, price: 6}
  - {desc: Photo book 30 × 30 cm, qty: 1, unit: pc, price: 89}
---

Thank you for the lovely shoot! For my services I am charging:
