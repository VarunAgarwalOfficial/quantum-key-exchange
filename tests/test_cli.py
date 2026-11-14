from qkx.cli import EXIT_ABORTED, EXIT_OK, main


def test_demo_delivers_and_prints_every_step(capsys):
    assert main(["demo", "héllo", "--seed", "1"]) == EXIT_OK
    out = capsys.readouterr().out
    for label in ("Alice bit", "Alice basis", "Bob basis", "Bob bit", "kept", "sifted", "sample"):
        assert label in out
    assert "error rate   0.0%" in out
    assert "Eve" not in out
    assert "Bob reads    héllo" in out


def test_demo_table_rows_are_aligned(capsys):
    main(["demo", "x", "--seed", "2", "--eve", "1"])
    rows = [line for line in capsys.readouterr().out.splitlines() if line.startswith("  ")][:7]
    assert [row.split()[0] for row in rows] == [
        "Alice",
        "Alice",
        "Eve",
        "Bob",
        "Bob",
        "kept",
        "error",
    ]
    assert len({len(row) for row in rows}) == 1


def test_demo_with_eve_aborts_without_sending(capsys):
    assert main(["demo", "top secret", "--seed", "1", "--eve", "1.0"]) == EXIT_ABORTED
    out = capsys.readouterr().out
    assert "ABORT" in out
    assert "not sent" in out
    assert "ciphertext" not in out
    assert "top secret" not in out


def test_demo_is_repeatable_with_a_seed(capsys):
    main(["demo", "same", "--seed", "9", "--eve", "0.3"])
    first = capsys.readouterr().out
    main(["demo", "same", "--seed", "9", "--eve", "0.3"])
    assert capsys.readouterr().out == first


def test_stats_prints_a_row_per_fraction(capsys):
    assert main(["stats", "--trials", "5", "--seed", "1", "--bytes", "4"]) == EXIT_OK
    lines = capsys.readouterr().out.splitlines()
    assert len(lines) == 3 + 11
    assert lines[3].split()[0] == "0%" and lines[3].endswith(" 0%")
    assert lines[-1].endswith("# 100%")
