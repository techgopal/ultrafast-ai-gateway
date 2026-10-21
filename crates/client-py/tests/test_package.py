def test_the_package_is_ufaigw():
    import ufaigw

    assert ufaigw.Client and ufaigw.AsyncClient
    assert ufaigw._native.__name__ == "ufaigw._native"
