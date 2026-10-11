---
template: invoice
sender:
  name: Alex Example IT Consulting
  street: 1 Sample Street
  zip: "04109"
  city: Leipzig
  phone: +49 170 1234567
  email: billing@example.com
  tax_id: 231/123/45678
  bank:
    name: Example Bank
    iban: DE12 3456 7890 1234 5678 90
    bic: EXMPDEXXX
recipient: |
  Example Ltd.
  Accounts Payable
  5 High Street
  10115 Berlin
number: 2026-017
customer_number: C-1042
service_period: September 2026
items:
  - desc: System architecture consulting
    detail: On-site workshops, 12–14 Sep 2026
    qty: 16
    unit: h
    price: 110
  - desc: Implementation of a **CI/CD pipeline**
    qty: 1
    unit: flat
    price: 1850
  - desc: Travel expenses
    qty: 1
    unit: flat
    price: 214.60
---

Thank you for your order. For the services provided I am charging:
