---
template: article
subtitle: Online appointment booking for the Sample Clinic
author: Alex Example
date: 2026-09-30
numbered: true
toc: true
font: serif
accent: 6A1B9A
---

# Requirements Specification

## Current situation

Appointments are only made by phone today. At peak times the clinic is hardly
reachable, and about 15 % of appointments are missed.

## Objective

Patients should be able to book, move and cancel appointments online around
the clock. The no-show rate should fall below 5 %.

## Functional requirements

| ID   | Requirement                                          | Priority |
|:-----|:-----------------------------------------------------|:--------:|
| F-01 | Show free slots by type of treatment                 | Must     |
| F-02 | Book an appointment with e-mail confirmation         | Must     |
| F-03 | Reminder 24 hours before the appointment             | Must     |
| F-04 | Move or cancel an appointment online                 | Should   |
| F-05 | Waiting list for slots that become free at short notice | Could |

## Non-functional requirements

- **Privacy:** processing exclusively on servers in the EU, GDPR compliant
- **Availability:** at least 99.5 % per month
- **Accessibility:** WCAG 2.2 level AA

## Acceptance criteria

1. All must-have requirements are implemented and tested.
2. A four-week pilot runs without critical defects.
