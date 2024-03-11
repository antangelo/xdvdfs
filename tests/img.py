import harness
import os

NAME_LEN = 18
FILE_BYTE_COUNT = 0xe + NAME_LEN
assert FILE_BYTE_COUNT == 32


def rand_file_name():
    import string
    import random
    return ''.join(random.choice(string.ascii_lowercase) for _ in range(NAME_LEN))

class EmptyFile(harness.TestCase):
    def __init__(self):
        super().__init__(name='EmptyFile')

    def set_up(self, dir):
        with open(dir + '/empty_file', 'w') as f:
            pass

class EmptyRoot(harness.TestCase):
    def __init__(self):
        super().__init__(name='EmptyRoot')

    def set_up(self, dir):
        pass

class EmptySubdir(harness.TestCase):
    def __init__(self):
        super().__init__(name='EmptySubdir')

    def set_up(self, dir):
        os.mkdir(dir + '/subdir')

class SpecialCharsInName(harness.TestCase):
    def __init__(self):
        super().__init__(name='SpecialCharsInName')

    def create_file(self, dir, name):
        with open(dir + '/' + name, 'w') as f:
            f.write(name)

    def set_up(self, dir):
        self.create_file(dir, 'Ü')
        self.create_file(dir, 'b')
        self.create_file(dir, 'ü')
        self.create_file(dir, 'á')

class ManyFiles(harness.TestCase):
    def __init__(self):
        super().__init__(name='ManyFiles')


    def set_up(self, dir):
        os.mkdir(dir + '/a')
        max_offset = 65536
        num_files = max_offset // FILE_BYTE_COUNT + 1
        for i in range(num_files):
            name = rand_file_name()
            with open(dir + '/a/' + name, 'w') as f:
                f.write('data')

class DirentSize2048(harness.TestCase):
    def __init__(self):
        super().__init__(name='DirentSize2048')

    def set_up(self, dir):
        os.mkdir(dir + '/a')
        with open(dir + '/b', 'w') as f:
            f.write('data')

        for i in range(2048 // FILE_BYTE_COUNT):
            name = rand_file_name()
            with open(dir + '/a/' + name, 'w') as f:
                f.write('data')
