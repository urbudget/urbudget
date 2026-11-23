// The engine's first template and its empty one, as `budget_templates` answers in English, for
// tests that mock the bridge (budget_templates.rs has the full list).
import type { BudgetTemplate } from "./types";

export const templatesFixture = (): BudgetTemplate[] => [
  {
    id: "everyday",
    name: "Everyday basics",
    description: "Bills, needs, wants and savings. A good fit for most people.",
    summary: "Bills, Needs, Wants, Savings · 16 categories",
    groups: [
      { name: "Bills", categories: ["Rent or mortgage", "Electricity", "Water", "Internet", "Phone"] },
      { name: "Needs", categories: ["Groceries", "Transportation", "Medical", "Insurance"] },
      { name: "Wants", categories: ["Dining out", "Entertainment", "Shopping", "Subscriptions"] },
      { name: "Savings", categories: ["Emergency fund", "Vacation", "Gifts"] },
    ],
  },
  {
    id: "student",
    name: "Student",
    description: "Tuition, living costs and a little fun on a tight budget.",
    summary: "School, Living, Fun, Savings · 11 categories",
    groups: [
      { name: "School", categories: ["Tuition", "Books and supplies"] },
      { name: "Living", categories: ["Rent or mortgage", "Groceries", "Transportation", "Phone"] },
      { name: "Fun", categories: ["Dining out", "Going out", "Subscriptions"] },
      { name: "Savings", categories: ["Emergency fund", "Next term"] },
    ],
  },
  { id: "empty", name: "No categories", description: "Make your own as you go.", summary: "Make your own as you go.", groups: [] },
];
