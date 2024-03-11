class TestCase:
    def __init__(self, name):
        self.name = name

    def set_up(self, dir):
        raise Exception('Test case not implemented')
