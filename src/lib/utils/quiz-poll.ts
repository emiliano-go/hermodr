export type QuizOptionRow = { id: number; text: string };
export type QuizScope = { account: string | null; chat: string | null; generation: number; requestKey?: string | number };

export function quizScopeMatches(left: QuizScope | null, right: QuizScope | null): boolean {
  if (!left || !right) return left === right;
  return left.account === right.account && left.chat === right.chat && left.generation === right.generation && left.requestKey === right.requestKey;
}

export function prepareQuiz(question: string, rows: readonly QuizOptionRow[], selected: number | null) {
  const filled = rows.map((row) => ({ ...row, text: row.text.trim() })).filter((row) => row.text);
  const options = filled.map((row) => row.text);
  const index = filled.findIndex((row) => row.id === selected);
  const error = !question.trim() ? "Enter a question." : options.length < 2 || options.length > 12 ? "Enter 2–12 answers."
    : new Set(options).size !== options.length ? "Answers must be different." : index < 0 ? "Choose one correct answer." : null;
  return { question: question.trim(), options, correctIndex: index < 0 ? null : index, error };
}

export function quizFeedback(correct: boolean | null | undefined): string {
  return correct === true ? "Your answer is correct." : correct === false ? "Your answer is incorrect." : "Answer result unavailable.";
}
