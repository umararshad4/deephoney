import unittest
from ocr_benchmark import distance, measure, numbers


class OcrMetricsTests(unittest.TestCase):
    def test_word_substitution_deletion_and_insertions_count(self):
        self.assertEqual(distance("one two three".split(), "one too".split()), 2)
        self.assertEqual(measure("one", "one two three")["word_error_rate"], 2)

    def test_numeric_damage_is_not_normalized_away(self):
        expected = "$1,250.50 -3.5% ($125.75) -42.00"
        actual = "$1250.50 3.5% $125.75 42.00"
        self.assertEqual(measure(expected, actual)["numbers_exact"]["matched"], 0)
        self.assertEqual(numbers("2024 $1,250.50 -3.5% ($125.75)"),
                         ["2024", "$1,250.50", "-3.5%", "($125.75)"])

    def test_literal_punctuation_is_not_a_percentage(self):
        metric = measure("12.5% -3.5%", "12.50/. -3.50/.")["numbers_exact"]
        self.assertEqual(metric["matched"], 0)
        self.assertEqual(metric["missing"], ["12.5%", "-3.5%"])
        self.assertEqual(numbers("0/. 12.50/. -3.50/."), ["0", "12.50", "-3.50"])

    def test_duplicate_values_and_omissions(self):
        metric = measure("125 125 175", "125 175 175")["numbers_exact"]
        self.assertEqual(metric["matched"], 2)
        self.assertEqual(metric["missing"], ["125"])
        self.assertEqual(metric["unexpected"], ["175"])

    def test_empty_output_has_zero_recall_and_no_precision(self):
        metric = measure("2025 evidence", "")
        self.assertEqual(metric["words"]["recall"], 0)
        self.assertIsNone(metric["words"]["precision"])
        self.assertEqual(metric["word_error_rate"], 1)


if __name__ == "__main__":
    unittest.main()
